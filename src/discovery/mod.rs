mod filesystem;
mod python;
#[cfg(test)]
mod tests;
mod types;

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use ignore::{DirEntry, WalkBuilder};

use pyo3::prelude::*;

use crate::workspace::{WorkCompletion, WorkControl, WorkStopped};

pub(crate) use filesystem::{find_ancestor_entry, read_text_files};
pub(crate) use types::{DiscoveryError, NamedFileRequest, NamedFileResult};
use types::{MAX_DEPTH, MAX_RESULTS, validate_entry_name};

pub(crate) fn register_module(module: &Bound<'_, PyModule>) -> PyResult<()> {
    python::register(module)
}

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

pub(crate) fn discover_named_files(
    root: &Path,
    request: &NamedFileRequest,
    control: &WorkControl,
) -> Result<NamedFileResult, DiscoveryError> {
    validate_request(request)?;
    let root = canonical_directory(root, "discovery root")?;
    let mut builder = WalkBuilder::new(&root);
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
    let mut completion = checkpoint_start(control)?;
    if completion == WorkCompletion::Complete {
        completion = collect_named_files(&root, request, control, builder, &mut paths)?;
    }
    paths.truncate(request.limit);

    Ok(NamedFileResult { paths, completion })
}

fn collect_named_files(
    root: &Path,
    request: &NamedFileRequest,
    control: &WorkControl,
    builder: WalkBuilder,
    paths: &mut Vec<String>,
) -> Result<WorkCompletion, DiscoveryError> {
    for (index, walked) in builder.build().enumerate() {
        match control.checkpoint_periodic(index + 1, 128) {
            Ok(()) => (),
            Err(WorkStopped::Cancelled) => return Err(DiscoveryError::Cancelled),
            Err(WorkStopped::Deadline) => return Ok(WorkCompletion::DeadlineReached),
        }
        let Ok(entry) = walked else {
            continue;
        };
        if entry.depth() == 0 || !entry.file_type().is_some_and(|kind| kind.is_dir()) {
            continue;
        }

        let candidate = entry.path().join(&request.filename);
        if !fs::metadata(&candidate).is_ok_and(|metadata| metadata.is_file()) {
            continue;
        }
        paths.push(relative_path(root, &candidate)?);
        if paths.len() > request.limit {
            return Ok(WorkCompletion::FileLimitReached);
        }
    }

    checkpoint_finish(control)
}

fn checkpoint_start(control: &WorkControl) -> Result<WorkCompletion, DiscoveryError> {
    match control.checkpoint() {
        Ok(()) => Ok(WorkCompletion::Complete),
        Err(WorkStopped::Cancelled) => Err(DiscoveryError::Cancelled),
        Err(WorkStopped::Deadline) => Ok(WorkCompletion::DeadlineReached),
    }
}

fn checkpoint_finish(control: &WorkControl) -> Result<WorkCompletion, DiscoveryError> {
    checkpoint_start(control)
}

fn canonical_directory(path: &Path, label: &str) -> Result<PathBuf, DiscoveryError> {
    let canonical = path.canonicalize().map_err(|error| {
        DiscoveryError::Path(format!(
            "cannot resolve {label} {}: {error}",
            path.display()
        ))
    })?;
    if !canonical.is_dir() {
        return Err(DiscoveryError::Path(format!("{label} must be a directory")));
    }

    Ok(canonical)
}

fn validate_request(request: &NamedFileRequest) -> Result<(), DiscoveryError> {
    validate_entry_name(&request.filename)?;
    if !(1..=MAX_DEPTH).contains(&request.max_depth) {
        return Err(DiscoveryError::Configuration(format!(
            "named file discovery depth must be between one and {MAX_DEPTH}"
        )));
    }
    if !(1..=MAX_RESULTS).contains(&request.limit) {
        return Err(DiscoveryError::Configuration(format!(
            "named file discovery limit must be between one and {MAX_RESULTS}"
        )));
    }

    Ok(())
}

fn relative_path(root: &Path, path: &Path) -> Result<String, DiscoveryError> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| DiscoveryError::Path("file discovery returned an external path".to_owned()))?;
    Ok(relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/"))
}

fn allowed_entry(entry: &DirEntry) -> bool {
    if entry.depth() == 0 || !entry.file_type().is_some_and(|kind| kind.is_dir()) {
        return true;
    }

    !EXCLUDED_DIRECTORY_SET.contains(entry.file_name().to_string_lossy().as_ref())
}
