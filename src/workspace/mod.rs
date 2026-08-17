mod content;
mod control;
mod evidence;
mod file_mutations;
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
mod path;
pub(crate) mod python;
mod replace;
mod scan;
#[cfg(test)]
mod tests;
mod types;
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
    WorkspaceFileRead, WorkspaceTextSerialization, inspect_text, read_content,
};
pub(crate) use control::{Cancellation, WorkControl, WorkStopped};
pub(crate) use hashline_types::{HashlineOperation, HashlineRegister, HashlineSection};
pub(crate) use observation_types::{LineRange, ObservationReceipt, RenderedLine};
pub(crate) use observations::ObservationLedger;
pub(crate) use patch::{parse_apply_patch, parse_structured_patch};
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

#[derive(Debug)]
pub(crate) enum WorkspaceError {
    Configuration(String),
    Path(String),
    Read(String),
    Encoding(String),
    Binary(String),
    Limit(String),
    ObservationNotFound(String),
    ObservationCollision(String),
    UnseenLine(String),
    ObservedLineChanged(String),
    Stale(String),
    EditMode(String),
    Patch(String),
    PartialCommit {
        landed: Vec<String>,
        pending: Vec<String>,
        changes: Vec<FileChange>,
    },
    Write(String),
    Cancelled,
    Closed,
    Deadline,
}

#[derive(Clone, Debug)]
pub(crate) struct Workspace {
    state: Arc<WorkspaceState>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct WorkspacePolicy {
    pub allow_fuzzy_replace: bool,
    pub fuzzy_replace_threshold: f64,
    pub max_read_bytes: u64,
    pub max_observation_file_bytes: u64,
    pub max_observation_entries: usize,
    pub max_observation_store_bytes: usize,
    pub create_parent_directories: bool,
}

impl Default for WorkspacePolicy {
    fn default() -> Self {
        Self {
            allow_fuzzy_replace: false,
            fuzzy_replace_threshold: 0.9,
            max_read_bytes: 4 * 1024 * 1024,
            max_observation_file_bytes: 64 * 1024 * 1024,
            max_observation_entries: 4096,
            max_observation_store_bytes: 8 * 1024 * 1024,
            create_parent_directories: false,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct PolicyGeneration {
    pub policy: WorkspacePolicy,
    pub generation: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct EditModeSelection {
    pub mode: String,
    pub generation: u64,
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
        Ok(Self {
            state: Arc::new(WorkspaceState {
                canonical_root: path::canonical_root(value)?,
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
        })
    }

    pub(crate) fn from_canonical(root: &Path) -> Self {
        Self {
            state: Arc::new(WorkspaceState {
                canonical_root: root.to_path_buf(),
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

    pub(crate) fn policy(&self) -> Result<PolicyGeneration, WorkspaceError> {
        self.ensure_open()?;
        Ok(self.lock(&self.state.policy)?.clone())
    }

    pub(crate) fn set_policy(
        &self,
        policy: WorkspacePolicy,
    ) -> Result<PolicyGeneration, WorkspaceError> {
        let _coordinator = self.write_guard()?;

        self.ensure_open()?;
        let mut current = self.lock(&self.state.policy)?;
        if current.policy != policy {
            current.policy = policy;
            current.generation = current.generation.saturating_add(1);
        }
        Ok(current.clone())
    }

    pub(crate) fn edit_mode(&self) -> Result<EditModeSelection, WorkspaceError> {
        self.ensure_open()?;
        Ok(self.lock(&self.state.edit_mode)?.clone())
    }

    pub(crate) fn set_edit_mode(&self, mode: &str) -> Result<EditModeSelection, WorkspaceError> {
        let _coordinator = self.write_guard()?;

        self.ensure_open()?;
        if !self.lock(&self.state.registered_edit_modes)?.contains(mode) {
            return Err(WorkspaceError::EditMode(format!(
                "workspace edit mode is not registered: {mode}"
            )));
        }
        let mut current = self.lock(&self.state.edit_mode)?;
        if current.mode != mode {
            current.mode = mode.to_owned();
            current.generation = current.generation.saturating_add(1);
        }
        Ok(current.clone())
    }

    pub(crate) fn register_edit_mode(&self, mode: &str) -> Result<(), WorkspaceError> {
        self.ensure_open()?;
        if !mode.contains('.') || mode.starts_with('.') || mode.ends_with('.') {
            return Err(WorkspaceError::EditMode(format!(
                "custom workspace edit mode must be namespaced: {mode}"
            )));
        }
        self.lock(&self.state.registered_edit_modes)?
            .insert(mode.to_owned());
        Ok(())
    }

    pub(crate) fn supports_edit_mode(&self, mode: &str) -> Result<bool, WorkspaceError> {
        self.ensure_open()?;
        Ok(self.lock(&self.state.registered_edit_modes)?.contains(mode))
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

    pub(crate) fn validate_mutation(
        &self,
        context: &MutationContext,
        expected_mode: &str,
    ) -> Result<(), WorkspaceError> {
        if context.mode != expected_mode {
            return Err(WorkspaceError::EditMode(format!(
                "captured edit mode does not match mutation: {} != {expected_mode}",
                context.mode
            )));
        }

        context.ensure_active()
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

    pub(crate) fn read_file(
        &self,
        path: &str,
        ranges: &[LineRange],
    ) -> Result<WorkspaceFileRead, WorkspaceError> {
        self.ensure_open()?;
        let path = path::normalize_relative(path)?;
        let policy = self.policy()?;
        let target = path::resolve_contained_file(self.root(), &path)?;
        let metadata = fs::metadata(&target)
            .map_err(|error| WorkspaceError::Read(format!("cannot inspect {path}: {error}")))?;
        let initial_total_bytes = metadata.len();
        let initial_complete_identity =
            initial_total_bytes <= policy.policy.max_observation_file_bytes;
        let control = WorkControl::new(Cancellation::new(), None);
        let content = read_content(
            &target,
            if initial_complete_identity {
                ReadExtent::Complete {
                    max_bytes: policy.policy.max_observation_file_bytes,
                }
            } else {
                ReadExtent::Prefix {
                    max_bytes: policy.policy.max_read_bytes,
                }
            },
            &control,
        )?;
        let total_bytes = content.total_bytes;
        let complete_identity = initial_complete_identity && content.complete;
        let text = if complete_identity {
            NormalizedText::decode(content.bytes)?
        } else {
            NormalizedText::decode_prefix(content.bytes)?
        };
        let total_lines = if complete_identity {
            text.total_lines()
        } else {
            inspect_text(&target)?
        };
        let normalized_ranges = normalize_read_ranges(ranges, text.total_lines())?;
        let lines = bounded_render(&text, &normalized_ranges, policy.policy.max_read_bytes);
        let visible_ranges = ranges_from_rendered(&lines);
        let complete_presentation = complete_identity
            && total_lines == lines.len()
            && (total_lines == 0
                || visible_ranges
                    == vec![LineRange {
                        start: 1,
                        end: total_lines,
                    }]);
        let observation = if complete_identity {
            let generation = self.file_generation(&path)?;
            Some(self.observations()?.record(
                &path,
                &text,
                generation,
                &lines,
                complete_presentation,
                (
                    policy.policy.max_observation_entries,
                    policy.policy.max_observation_store_bytes,
                ),
            )?)
        } else {
            None
        };
        let serialization = complete_identity.then_some(WorkspaceTextSerialization {
            bom: text.serialization.bom,
            line_ending: text.serialization.line_ending,
            terminal_newline: text.source.ends_with('\n'),
        });

        Ok(WorkspaceFileRead {
            path: path.to_owned(),
            observation,
            lines,
            total_lines,
            complete_presentation,
            editable: complete_identity,
            total_bytes,
            observation_limit: policy.policy.max_observation_file_bytes,
            serialization,
        })
    }

    pub(crate) fn list_directory(
        &self,
        path: &str,
        depth: usize,
    ) -> Result<WorkspaceDirectoryRead, WorkspaceError> {
        let path = path::normalize_relative_directory(path)?;
        self.ensure_open()?;
        if !(1..=2).contains(&depth) {
            return Err(WorkspaceError::Limit(
                "workspace directory depth must be between one and two".to_owned(),
            ));
        }
        let target = path::resolve_contained_directory(self.root(), &path)?;
        let mut entries = Vec::new();
        let mut truncated = false;
        collect_directory_entries(
            self.root(),
            &target,
            depth,
            4096,
            &mut entries,
            &mut truncated,
        )?;
        entries.sort_by(|left, right| left.path.cmp(&right.path));

        Ok(WorkspaceDirectoryRead {
            path,
            entries,
            truncated,
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

fn normalize_read_ranges(
    ranges: &[LineRange],
    total_lines: usize,
) -> Result<Vec<LineRange>, WorkspaceError> {
    if ranges.is_empty() {
        return Ok(if total_lines == 0 {
            Vec::new()
        } else {
            vec![LineRange {
                start: 1,
                end: total_lines,
            }]
        });
    }

    let mut normalized = ranges
        .iter()
        .map(|range| {
            if range.start == 0 || range.end < range.start {
                return Err(WorkspaceError::Read(
                    "workspace read ranges must be one-based and ordered".to_owned(),
                ));
            }
            Ok(LineRange {
                start: range.start,
                end: range.end.min(total_lines),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    normalized.retain(|range| range.start <= total_lines);
    normalized.sort_by_key(|range| range.start);
    let mut merged: Vec<LineRange> = Vec::new();
    for range in normalized {
        match merged.last_mut() {
            Some(previous) if range.start <= previous.end => {
                return Err(WorkspaceError::Read(
                    "workspace read ranges cannot overlap".to_owned(),
                ));
            }
            Some(previous) if range.start == previous.end.saturating_add(1) => {
                previous.end = range.end;
            }
            _ => merged.push(range),
        }
    }
    Ok(merged)
}

fn bounded_render(
    text: &NormalizedText,
    ranges: &[LineRange],
    max_bytes: u64,
) -> Vec<RenderedLine> {
    let mut lines = Vec::new();
    let mut used = 0_u64;
    for range in ranges {
        for number in range.start..=range.end {
            let Some(source) = text.line(number) else {
                continue;
            };
            let line_bytes = source.len() as u64;
            if used.saturating_add(line_bytes) > max_bytes {
                return lines;
            }
            used = used.saturating_add(line_bytes);
            lines.push(RenderedLine {
                number,
                short_hash: format!("{:02X}", line_hash::short_line_hash(source.as_bytes())),
                text: source.to_owned(),
            });
        }
    }
    lines
}

fn ranges_from_rendered(lines: &[RenderedLine]) -> Vec<LineRange> {
    let mut ranges: Vec<LineRange> = Vec::new();
    for line in lines {
        match ranges.last_mut() {
            Some(range) if line.number == range.end.saturating_add(1) => range.end = line.number,
            _ => ranges.push(LineRange {
                start: line.number,
                end: line.number,
            }),
        }
    }
    ranges
}

fn collect_directory_entries(
    root: &Path,
    directory: &Path,
    depth: usize,
    limit: usize,
    entries: &mut Vec<WorkspaceDirectoryEntry>,
    truncated: &mut bool,
) -> Result<(), WorkspaceError> {
    let reader = fs::read_dir(directory).map_err(|error| {
        WorkspaceError::Read(format!("cannot list workspace directory: {error}"))
    })?;
    let mut children = reader.collect::<Result<Vec<_>, _>>().map_err(|error| {
        WorkspaceError::Read(format!("cannot list workspace directory: {error}"))
    })?;
    children.sort_by_key(std::fs::DirEntry::file_name);
    for child in children {
        if entries.len() == limit {
            *truncated = true;
            return Ok(());
        }
        let target = child.path();
        let metadata = fs::symlink_metadata(&target).map_err(|error| {
            WorkspaceError::Read(format!("cannot inspect workspace entry: {error}"))
        })?;
        let relative = path::relative_path(root, &target)?;
        let file_type = metadata.file_type();
        let kind = if file_type.is_symlink() {
            "symlink"
        } else if metadata.is_dir() {
            "directory"
        } else {
            "file"
        };
        entries.push(WorkspaceDirectoryEntry {
            path: relative,
            kind: kind.to_owned(),
            size: metadata.is_file().then_some(metadata.len()),
        });
        if depth > 1 && metadata.is_dir() {
            collect_directory_entries(root, &target, depth - 1, limit, entries, truncated)?;
            if *truncated {
                return Ok(());
            }
        }
    }
    Ok(())
}
fn registered_edit_modes() -> HashSet<String> {
    ["hashline", "replace", "patch", "apply_patch"]
        .into_iter()
        .map(str::to_owned)
        .collect()
}
