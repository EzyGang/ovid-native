use std::{collections::HashSet, path::Path, sync::Arc};

use ignore::{DirEntry, WalkBuilder};

use crate::workspace::path::relative_path;
use crate::workspace::scan_entry::ScanCandidate;
use crate::workspace::selection::{
    Selection, build_selections, explicitly_includes_node_modules, relevant_directory, selected,
};
use crate::workspace::{
    ScanOrder, ScanRequest, ScanResult, WorkCompletion, WorkControl, WorkStopped, WorkspaceError,
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

    if !within_deadline(control.checkpoint())? {
        return Ok(ScanResult {
            entries,
            scanned_entries,
            skipped_entries,
            completion: WorkCompletion::DeadlineReached,
        });
    }
    for walked in builder.build() {
        walked_entries += 1;
        if !within_deadline(control.checkpoint_periodic(walked_entries, 128))? {
            completion = WorkCompletion::DeadlineReached;
            break;
        }
        let entry = walked.map_err(|_| WorkspaceError::Path("workspace scan failed".to_owned()))?;
        let Some(candidate) = ScanCandidate::select(root, entry, &selections, request.file_kind)?
        else {
            skipped_entries += 1;
            continue;
        };
        if !canonical_paths.insert(candidate.identity(root)?) {
            skipped_entries += 1;
            continue;
        }
        scanned_entries += 1;
        if entries.len() == request.max_files {
            completion = WorkCompletion::FileLimitReached;
            break;
        }
        entries.push(candidate.into_entry(request.metadata)?);
    }
    if completion == WorkCompletion::Complete && !within_deadline(control.checkpoint())? {
        completion = WorkCompletion::DeadlineReached;
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

fn relevant_entry(root: &Path, entry: &DirEntry, selections: &[Selection]) -> bool {
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

fn within_deadline(result: Result<(), WorkStopped>) -> Result<bool, WorkspaceError> {
    match result {
        Ok(()) => Ok(true),
        Err(WorkStopped::Deadline) => Ok(false),
        Err(WorkStopped::Cancelled) => Err(WorkspaceError::Cancelled),
    }
}
