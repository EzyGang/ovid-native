use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use ignore::{DirEntry, WalkBuilder};

use crate::workspace::WorkspaceError;
use crate::workspace::control::{WorkControl, WorkStopped};
use crate::workspace::path::{
    build_selections, explicitly_includes_node_modules, explicitly_selected_file, relative_path,
    relevant_directory, selected,
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
    let selections = Arc::new(build_selections(root, &request.selections)?);
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
    let filter_root = root.to_path_buf();
    let filter_selections = Arc::clone(&selections);
    builder.filter_entry(move |entry| {
        allowed_entry(entry, include_node_modules)
            && relevant_entry(&filter_root, entry, &filter_selections)
    });

    let mut entries = Vec::new();
    let mut canonical_paths = HashSet::new();
    let mut scanned_entries = 0;
    let mut skipped_entries = 0;
    let mut completion = WorkCompletion::Complete;
    let mut walked_entries = 0;

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
        walked_entries += 1;
        match control.checkpoint_periodic(walked_entries, 128) {
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

        let native_file_type = entry.file_type().ok_or_else(|| {
            WorkspaceError::Path(format!("cannot classify workspace path {relative}"))
        })?;
        let is_symlink = native_file_type.is_symlink();
        if is_symlink && !explicitly_selected_file(&relative, &selections) {
            skipped_entries += 1;
            continue;
        }

        let followed_metadata = if is_symlink {
            Some(fs::metadata(entry.path()).map_err(|error| {
                WorkspaceError::Path(format!("cannot inspect workspace path {relative}: {error}"))
            })?)
        } else {
            None
        };
        let file_type = match (
            followed_metadata
                .as_ref()
                .is_some_and(std::fs::Metadata::is_file)
                || native_file_type.is_file(),
            followed_metadata
                .as_ref()
                .is_some_and(std::fs::Metadata::is_dir)
                || native_file_type.is_dir(),
        ) {
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
        let identity = if is_symlink {
            let canonical = entry.path().canonicalize().map_err(|error| {
                WorkspaceError::Path(format!("cannot resolve workspace path {relative}: {error}"))
            })?;
            if !canonical.starts_with(root) {
                return Err(WorkspaceError::Path(format!(
                    "path resolves outside the workspace: {relative}"
                )));
            }
            canonical
        } else {
            entry.path().to_path_buf()
        };
        if !canonical_paths.insert(identity) {
            skipped_entries += 1;
            continue;
        }

        scanned_entries += 1;
        if entries.len() == request.max_files {
            completion = WorkCompletion::FileLimitReached;
            break;
        }
        let needs_metadata = match (request.metadata, file_type) {
            (MetadataLevel::Minimal, _) | (MetadataLevel::Size, WorkspaceFileType::Directory) => {
                false
            }
            (MetadataLevel::Size | MetadataLevel::Full, WorkspaceFileType::File)
            | (MetadataLevel::Full, WorkspaceFileType::Directory) => true,
        };
        let metadata = match (followed_metadata, needs_metadata) {
            (Some(metadata), _) => Some(metadata),
            (None, true) => Some(entry.metadata().map_err(|error| {
                WorkspaceError::Path(format!("cannot inspect workspace path {relative}: {error}"))
            })?),
            (None, false) => None,
        };
        let size =
            if request.metadata != MetadataLevel::Minimal && file_type == WorkspaceFileType::File {
                metadata.as_ref().map(std::fs::Metadata::len)
            } else {
                None
            };
        let modified = if request.metadata == MetadataLevel::Full {
            metadata.as_ref().and_then(|value| value.modified().ok())
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
    if completion == WorkCompletion::Complete {
        match control.checkpoint() {
            Ok(()) => (),
            Err(WorkStopped::Cancelled) => return Err(WorkspaceError::Cancelled),
            Err(WorkStopped::Deadline) => completion = WorkCompletion::DeadlineReached,
        }
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

fn relevant_entry(
    root: &Path,
    entry: &DirEntry,
    selections: &[crate::workspace::path::Selection],
) -> bool {
    let Ok(relative) = relative_path(root, entry.path()) else {
        return false;
    };
    let Some(file_type) = entry.file_type() else {
        return false;
    };

    if file_type.is_dir() {
        relevant_directory(&relative, selections)
    } else {
        selected(&relative, selections)
    }
}
