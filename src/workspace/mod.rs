mod content;
mod control;
mod directory_read;
mod directory_tree;
#[cfg(test)]
mod directory_tree_tests;
mod error;
mod evidence;
mod file_mutations;
mod file_read;
mod hashline;
mod hashline_apply;
mod hashline_block;
mod hashline_locator;
mod hashline_plan;
#[cfg(test)]
mod hashline_tests;
mod hashline_types;
mod line_hash;
#[cfg(test)]
mod observation_tests;
mod observation_types;
mod observations;
mod patch;
mod patch_commit;
mod patch_plan;
mod path;
mod policy;
pub(crate) mod python;
mod read_ranges;
mod replace;
mod scan;
mod scan_entry;
mod selection;
#[cfg(test)]
mod tests;
mod text_similarity;
mod types;
mod unified_diff;
#[cfg(test)]
mod workflow_tests;
mod workflows;
mod write;

use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use fs2::FileExt;

pub(crate) use content::{
    LineEnding, NormalizedText, ReadExtent, WorkspaceDirectoryEntry, WorkspaceDirectoryRead,
    WorkspaceFileRead, WorkspaceTextSerialization, read_content,
};
pub(crate) use control::{Cancellation, WorkControl, WorkStopped};
pub(crate) use directory_tree::{WorkspaceDirectoryTreeLine, WorkspaceDirectoryTreeRead};
pub(crate) use error::WorkspaceError;
pub(crate) use hashline_types::{HashlineOperation, HashlineRegister, HashlineSection};
pub(crate) use observation_types::{LineRange, ObservationReceipt, RenderedLine};
pub(crate) use observations::ObservationLedger;
pub(crate) use patch::{parse_apply_patch, parse_structured_patch};
pub(crate) use policy::{EditModeSelection, PolicyGeneration, WorkspacePolicy};
pub(crate) use python::NativeWorkspace;
pub(crate) use types::{
    MetadataLevel, ScanFileKind, ScanOrder, ScanRequest, ScanResult, WorkCompletion,
    WorkspaceEntry, WorkspaceFileType,
};
pub(crate) use workflows::MutationContext;
pub(crate) use write::{
    EditResult, FileChange, FileIdentity, PostEditSource, atomic_replace_path, create_file,
    ensure_current_file, file_identity, move_file_noclobber, preflight_write, replace_file, sha256,
};

#[derive(Clone, Debug)]
pub(crate) struct Workspace {
    state: Arc<WorkspaceState>,
}

#[derive(Debug)]
struct WorkspaceState {
    canonical_root: std::path::PathBuf,
    closed: AtomicBool,
    revision: AtomicU64,
    write_coordinator: Mutex<()>,
    observations: Mutex<ObservationLedger>,
    named_registers: Mutex<HashMap<String, HashlineRegister>>,
    file_generations: Mutex<HashMap<String, u64>>,
    policy: Mutex<PolicyGeneration>,
    edit_mode: Mutex<EditModeSelection>,
    registered_edit_modes: Mutex<HashSet<String>>,
}

#[derive(Debug)]
pub(crate) struct WorkspaceWriteGuard<'a> {
    _coordinator: MutexGuard<'a, ()>,
    _file: fs::File,
}

impl Workspace {
    pub(crate) fn new(value: &str) -> Result<Self, WorkspaceError> {
        Ok(Self::with_root(path::canonical_root(value)?))
    }

    pub(crate) fn from_canonical(root: &Path) -> Self {
        Self::with_root(root.to_path_buf())
    }

    fn with_root(root: std::path::PathBuf) -> Self {
        Self {
            state: Arc::new(WorkspaceState {
                canonical_root: root,
                closed: AtomicBool::new(false),
                revision: AtomicU64::new(1),
                write_coordinator: Mutex::new(()),
                observations: Mutex::new(ObservationLedger::default()),
                named_registers: Mutex::new(HashMap::new()),
                file_generations: Mutex::new(HashMap::new()),
                policy: Mutex::new(PolicyGeneration {
                    policy: WorkspacePolicy::default(),
                    generation: 1,
                }),
                edit_mode: Mutex::new(EditModeSelection {
                    mode: "apply_patch".to_owned(),
                    generation: 1,
                }),
                registered_edit_modes: Mutex::new(registered_edit_modes()),
            }),
        }
    }

    pub(crate) fn root(&self) -> &Path {
        &self.state.canonical_root
    }

    pub(crate) fn ensure_open(&self) -> Result<(), WorkspaceError> {
        if self.state.closed.load(Ordering::Acquire) {
            return Err(WorkspaceError::Closed);
        }

        Ok(())
    }

    pub(crate) fn close(&self) {
        self.state.closed.store(true, Ordering::Release);
        if let Ok(mut registers) = self.state.named_registers.lock() {
            registers.clear();
        }
    }

    pub(crate) fn is_closed(&self) -> bool {
        self.state.closed.load(Ordering::Acquire)
    }

    pub(crate) fn revision(&self) -> u64 {
        self.state.revision.load(Ordering::Acquire)
    }

    pub(crate) fn mark_changed(&self) -> u64 {
        self.state.revision.fetch_add(1, Ordering::AcqRel) + 1
    }

    pub(crate) fn write_guard(&self) -> Result<WorkspaceWriteGuard<'_>, WorkspaceError> {
        self.ensure_open()?;
        let coordinator = self.lock(&self.state.write_coordinator)?;
        let name = format!(
            "ovid-native-workspace-{}.lock",
            sha256(self.root().to_string_lossy().as_bytes())
        );
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(std::env::temp_dir().join(name))
            .map_err(|error| {
                WorkspaceError::Write(format!("cannot open workspace lock: {error}"))
            })?;
        FileExt::lock_exclusive(&file)
            .map_err(|error| WorkspaceError::Write(format!("cannot lock workspace: {error}")))?;

        Ok(WorkspaceWriteGuard {
            _coordinator: coordinator,
            _file: file,
        })
    }

    pub(crate) fn observations(&self) -> Result<MutexGuard<'_, ObservationLedger>, WorkspaceError> {
        self.ensure_open()?;
        self.lock(&self.state.observations)
    }

    pub(crate) fn named_registers(
        &self,
    ) -> Result<HashMap<String, HashlineRegister>, WorkspaceError> {
        Ok(self.lock(&self.state.named_registers)?.clone())
    }

    pub(crate) fn replace_named_registers(
        &self,
        registers: HashMap<String, HashlineRegister>,
    ) -> Result<(), WorkspaceError> {
        *self.lock(&self.state.named_registers)? = registers;
        Ok(())
    }

    pub(crate) fn file_generation(&self, path: &str) -> Result<u64, WorkspaceError> {
        let mut generations = self.lock(&self.state.file_generations)?;
        Ok(*generations.entry(path.to_owned()).or_insert(1))
    }

    pub(crate) fn mark_file_changed(&self, path: &str) -> Result<(u64, u64), WorkspaceError> {
        let mut generations = self.lock(&self.state.file_generations)?;
        let generation = generations.entry(path.to_owned()).or_insert(1);
        *generation = generation.saturating_add(1);
        Ok((*generation, self.mark_changed()))
    }

    fn lock<'a, T>(&self, mutex: &'a Mutex<T>) -> Result<MutexGuard<'a, T>, WorkspaceError> {
        mutex.lock().map_err(|_| {
            WorkspaceError::Configuration("workspace state lock is unavailable".to_owned())
        })
    }

    pub(crate) fn scan(
        &self,
        request: &ScanRequest,
        control: &WorkControl,
    ) -> Result<ScanResult, WorkspaceError> {
        self.ensure_open()?;
        scan::scan(&self.state.canonical_root, request, control)
    }
}

fn registered_edit_modes() -> HashSet<String> {
    ["hashline", "replace", "patch", "apply_patch"]
        .into_iter()
        .map(str::to_owned)
        .collect()
}
