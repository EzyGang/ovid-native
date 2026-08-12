use std::collections::HashSet;
use std::fs;
use std::path::Path;

use ignore::{DirEntry, WalkBuilder};

use crate::workspace::WorkspaceError;
use crate::workspace::control::{WorkControl, WorkStopped};
use crate::workspace::path::{
    build_selections, explicitly_includes_node_modules, relative_path, selected,
};
use crate::workspace::types::{
    MetadataLevel, ScanFileKind, ScanOrder, ScanRequest, ScanResult, WorkCompletion,
    WorkspaceEntry, WorkspaceFileType,
};

pub(crate) fn scan(
    root: &Path,
    request: &ScanRequest,
    control: &WorkControl,
) -> Result<ScanResult, WorkspaceError> {
    let selections = build_selections(root, &request.selections)?;
    let include_node_modules =
        request.include_node_modules || explicitly_includes_node_modules(&request.selections);
    let mut builder = WalkBuilder::new(root);
    builder
        .follow_links(false)
        .hidden(!request.include_hidden)
        .ignore(request.respect_gitignore)
        .git_ignore(request.respect_gitignore)
        .git_exclude(request.respect_gitignore)
        .git_global(false)
        .require_git(false)
        .parents(false);
    if request.order == ScanOrder::Path {
        builder.sort_by_file_path(|left, right| left.cmp(right));
    }
    builder.filter_entry(move |entry| allowed_entry(entry, include_node_modules));

    let mut entries = Vec::new();
    let mut canonical_paths = HashSet::new();
    let mut scanned_entries = 0;
    let mut skipped_entries = 0;
    let mut completion = WorkCompletion::Complete;

    match control.checkpoint() {
        Ok(()) => (),
        Err(WorkStopped::Cancelled) => return Err(WorkspaceError::Cancelled),
        Err(WorkStopped::Deadline) => {
            return Ok(ScanResult {
                entries,
                scanned_entries,
                skipped_entries,
                completion: WorkCompletion::DeadlineReached,
            });
        }
    }
    for walked in builder.build() {
        match control.checkpoint() {
            Ok(()) => (),
            Err(WorkStopped::Cancelled) => return Err(WorkspaceError::Cancelled),
            Err(WorkStopped::Deadline) => {
                completion = WorkCompletion::DeadlineReached;
                break;
            }
        }

        let entry = walked.map_err(|_| WorkspaceError::Path("workspace scan failed".to_owned()))?;
        let relative = relative_path(root, entry.path())?;
        if relative.is_empty() || !selected(&relative, &selections) {
            skipped_entries += 1;
            continue;
        }

        let symlink_metadata = fs::symlink_metadata(entry.path()).map_err(|error| {
            WorkspaceError::Path(format!("cannot inspect workspace path {relative}: {error}"))
        })?;
        let metadata = fs::metadata(entry.path()).map_err(|error| {
            WorkspaceError::Path(format!("cannot inspect workspace path {relative}: {error}"))
        })?;
        if symlink_metadata.file_type().is_symlink() && metadata.is_dir() {
            skipped_entries += 1;
            continue;
        }

        let file_type = match (metadata.is_file(), metadata.is_dir()) {
            (true, false) if request.file_kind != ScanFileKind::Directories => {
                WorkspaceFileType::File
            }
            (false, true) if request.file_kind != ScanFileKind::Files => {
                WorkspaceFileType::Directory
            }
            _ => {
                skipped_entries += 1;
                continue;
            }
        };
        let canonical = entry.path().canonicalize().map_err(|error| {
            WorkspaceError::Path(format!("cannot resolve workspace path {relative}: {error}"))
        })?;
        if !canonical.starts_with(root) {
            return Err(WorkspaceError::Path(format!(
                "path resolves outside the workspace: {relative}"
            )));
        }
        if !canonical_paths.insert(canonical) {
            skipped_entries += 1;
            continue;
        }

        scanned_entries += 1;
        if entries.len() == request.max_files {
            completion = WorkCompletion::FileLimitReached;
            break;
        }
        let size = match (request.metadata, file_type) {
            (MetadataLevel::Minimal, _) | (_, WorkspaceFileType::Directory) => None,
            (MetadataLevel::Size | MetadataLevel::Full, WorkspaceFileType::File) => {
                Some(metadata.len())
            }
        };
        let modified = if request.metadata == MetadataLevel::Full {
            metadata.modified().ok()
        } else {
            None
        };
        entries.push(WorkspaceEntry {
            path: entry.into_path(),
            relative,
            file_type,
            size,
            modified,
        });
    }

    if request.order == ScanOrder::Path {
        entries.sort_by(|left, right| left.relative.cmp(&right.relative));
    }

    Ok(ScanResult {
        entries,
        scanned_entries,
        skipped_entries,
        completion,
    })
}

fn allowed_entry(entry: &DirEntry, include_node_modules: bool) -> bool {
    let name = entry.file_name().to_string_lossy();
    name != ".git" && (include_node_modules || name != "node_modules")
}
