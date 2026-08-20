use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::sync::LazyLock;

use ignore::{DirEntry, WalkBuilder};

use crate::workspace::control::{WorkControl, WorkStopped};
use crate::workspace::path::relative_path;
use crate::workspace::{WorkCompletion, WorkspaceError};

const MAX_DEPTH: usize = 64;
const MAX_RESULTS: usize = 10_000;

const EXCLUDED_DIRECTORIES: &[&str] = &[
    "node_modules",
    ".git",
    ".next",
    "dist",
    "build",
    "target",
    ".venv",
    ".cache",
    ".turbo",
    ".parcel-cache",
    "coverage",
    "__pycache__",
    ".mypy_cache",
    ".pytest_cache",
    ".ruff_cache",
    ".tox",
];

static EXCLUDED_DIRECTORY_SET: LazyLock<HashSet<&'static str>> =
    LazyLock::new(|| EXCLUDED_DIRECTORIES.iter().copied().collect());

#[derive(Clone, Debug)]
pub(crate) struct DiscoveryRequest {
    pub filename: String,
    pub max_depth: usize,
    pub limit: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct DiscoveryResult {
    pub paths: Vec<String>,
    pub completion: WorkCompletion,
}

pub(crate) fn discover_files(
    root: &Path,
    request: &DiscoveryRequest,
    control: &WorkControl,
) -> Result<DiscoveryResult, WorkspaceError> {
    validate_request(request)?;
    let mut builder = WalkBuilder::new(root);
    builder
        .follow_links(false)
        .hidden(true)
        .ignore(true)
        .git_ignore(true)
        .git_exclude(true)
        .git_global(false)
        .require_git(false)
        .parents(true)
        .max_depth(Some(request.max_depth))
        .sort_by_file_path(|left, right| left.cmp(right))
        .filter_entry(allowed_entry);

    let mut paths = Vec::new();
    let mut completion = WorkCompletion::Complete;
    match control.checkpoint() {
        Ok(()) => (),
        Err(WorkStopped::Cancelled) => return Err(WorkspaceError::Cancelled),
        Err(WorkStopped::Deadline) => {
            return Ok(DiscoveryResult {
                paths,
                completion: WorkCompletion::DeadlineReached,
            });
        }
    }

    for (index, walked) in builder.build().enumerate() {
        match control.checkpoint_periodic(index + 1, 128) {
            Ok(()) => (),
            Err(WorkStopped::Cancelled) => return Err(WorkspaceError::Cancelled),
            Err(WorkStopped::Deadline) => {
                completion = WorkCompletion::DeadlineReached;
                break;
            }
        }
        let Ok(entry) = walked else {
            continue;
        };
        if entry.depth() == 0
            || !entry
                .file_type()
                .is_some_and(|file_type| file_type.is_dir())
        {
            continue;
        }

        let candidate = entry.path().join(&request.filename);
        if !fs::metadata(&candidate).is_ok_and(|metadata| metadata.is_file()) {
            continue;
        }
        paths.push(relative_path(root, &candidate)?);
        if paths.len() > request.limit {
            completion = WorkCompletion::FileLimitReached;
            break;
        }
    }
    if completion == WorkCompletion::Complete {
        match control.checkpoint() {
            Ok(()) => (),
            Err(WorkStopped::Cancelled) => return Err(WorkspaceError::Cancelled),
            Err(WorkStopped::Deadline) => completion = WorkCompletion::DeadlineReached,
        }
    }

    paths.truncate(request.limit);
    Ok(DiscoveryResult { paths, completion })
}

fn validate_request(request: &DiscoveryRequest) -> Result<(), WorkspaceError> {
    if request.filename.is_empty()
        || request.filename == "."
        || request.filename == ".."
        || request.filename.contains(['/', '\\', '\0'])
    {
        return Err(WorkspaceError::Configuration(
            "workspace discovery filename must be one file name".to_owned(),
        ));
    }
    if !(1..=MAX_DEPTH).contains(&request.max_depth) {
        return Err(WorkspaceError::Limit(format!(
            "workspace discovery depth must be between one and {MAX_DEPTH}",
        )));
    }
    if !(1..=MAX_RESULTS).contains(&request.limit) {
        return Err(WorkspaceError::Limit(format!(
            "workspace discovery limit must be between one and {MAX_RESULTS}",
        )));
    }

    Ok(())
}

fn allowed_entry(entry: &DirEntry) -> bool {
    if entry.depth() == 0 {
        return true;
    }
    if !entry
        .file_type()
        .is_some_and(|file_type| file_type.is_dir())
    {
        return true;
    }

    !EXCLUDED_DIRECTORY_SET.contains(entry.file_name().to_string_lossy().as_ref())
}
