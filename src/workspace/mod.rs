mod content;
mod control;
mod path;
pub(crate) mod python;
mod scan;
#[cfg(test)]
mod tests;
mod types;
mod write;

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

pub(crate) use content::{ReadExtent, read_content};
pub(crate) use control::{Cancellation, WorkControl, WorkStopped};
pub(crate) use python::NativeWorkspace;
pub(crate) use types::{
    MetadataLevel, ScanFileKind, ScanOrder, ScanRequest, ScanResult, WorkCompletion,
    WorkspaceEntry, WorkspaceFileType,
};
pub(crate) use write::{preflight_write, replace_file, sha256};

#[derive(Debug)]
pub(crate) enum WorkspaceError {
    Configuration(String),
    Path(String),
    Read(String),
    Stale(String),
    Write(String),
    Cancelled,
    Closed,
    Deadline,
}

#[derive(Clone, Debug)]
pub(crate) struct Workspace {
    state: Arc<WorkspaceState>,
}

#[derive(Debug)]
struct WorkspaceState {
    canonical_root: std::path::PathBuf,
    closed: AtomicBool,
    revision: AtomicU64,
}

impl Workspace {
    pub(crate) fn new(value: &str) -> Result<Self, WorkspaceError> {
        Ok(Self {
            state: Arc::new(WorkspaceState {
                canonical_root: path::canonical_root(value)?,
                closed: AtomicBool::new(false),
                revision: AtomicU64::new(1),
            }),
        })
    }

    pub(crate) fn from_canonical(root: &Path) -> Self {
        Self {
            state: Arc::new(WorkspaceState {
                canonical_root: root.to_path_buf(),
                closed: AtomicBool::new(false),
                revision: AtomicU64::new(1),
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

    pub(crate) fn scan(
        &self,
        request: &ScanRequest,
        control: &WorkControl,
    ) -> Result<ScanResult, WorkspaceError> {
        self.ensure_open()?;
        scan::scan(&self.state.canonical_root, request, control)
    }
}
