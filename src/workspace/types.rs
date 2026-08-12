use std::path::PathBuf;
use std::time::SystemTime;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ScanFileKind {
    Files,
    Directories,
    FilesAndDirectories,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MetadataLevel {
    Minimal,
    Size,
    Full,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ScanOrder {
    Path,
    Unordered,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WorkspaceFileType {
    File,
    Directory,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WorkCompletion {
    Complete,
    FileLimitReached,
    DeadlineReached,
}

#[derive(Clone, Debug)]
pub(crate) struct ScanRequest {
    pub selections: Vec<String>,
    pub include_hidden: bool,
    pub respect_gitignore: bool,
    pub include_node_modules: bool,
    pub file_kind: ScanFileKind,
    pub metadata: MetadataLevel,
    pub order: ScanOrder,
    pub max_files: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct WorkspaceEntry {
    pub path: PathBuf,
    pub relative: String,
    pub file_type: WorkspaceFileType,
    pub size: Option<u64>,
    pub modified: Option<SystemTime>,
}

#[derive(Clone, Debug)]
pub(crate) struct ScanResult {
    pub entries: Vec<WorkspaceEntry>,
    pub scanned_entries: usize,
    pub skipped_entries: usize,
    pub completion: WorkCompletion,
}
