mod content;
mod control;
mod path;
mod scan;
#[cfg(test)]
mod tests;
mod types;
mod write;

use std::path::Path;

pub(crate) use content::{ReadExtent, read_content};
pub(crate) use control::{Cancellation, WorkControl, WorkStopped};
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
    Deadline,
}

#[derive(Clone, Debug)]
pub(crate) struct Workspace {
    canonical_root: std::path::PathBuf,
}

impl Workspace {
    pub(crate) fn new(value: &str) -> Result<Self, WorkspaceError> {
        Ok(Self {
            canonical_root: path::canonical_root(value)?,
        })
    }

    pub(crate) fn from_canonical(root: &Path) -> Self {
        Self {
            canonical_root: root.to_path_buf(),
        }
    }

    pub(crate) fn root(&self) -> &Path {
        &self.canonical_root
    }

    pub(crate) fn scan(
        &self,
        request: &ScanRequest,
        control: &WorkControl,
    ) -> Result<ScanResult, WorkspaceError> {
        scan::scan(&self.canonical_root, request, control)
    }
}
