use std::time::{Duration, UNIX_EPOCH};

use crate::search::SearchError;
use crate::search::types::{GlobRequest, NativeGlobMatch, NativeGlobResult};
use crate::workspace::{
    MetadataLevel, ScanFileKind, ScanOrder, ScanRequest, WorkCompletion, WorkControl, Workspace,
    WorkspaceFileType,
};

pub(crate) fn glob(
    workspace: &Workspace,
    request: GlobRequest,
) -> Result<NativeGlobResult, SearchError> {
    let file_kind = match request.file_type.as_str() {
        "file" => ScanFileKind::Files,
        "directory" => ScanFileKind::Directories,
        "any" => ScanFileKind::FilesAndDirectories,
        value => {
            return Err(SearchError::Configuration(format!(
                "invalid glob file type: {value}"
            )));
        }
    };
    let (metadata, scan_order) = match request.order.as_str() {
        "path" => (MetadataLevel::Minimal, ScanOrder::Path),
        "modified_desc" => (MetadataLevel::Full, ScanOrder::Unordered),
        value => {
            return Err(SearchError::Configuration(format!(
                "invalid glob order: {value}"
            )));
        }
    };
    let control = WorkControl::new(
        request.cancellation,
        Some(Duration::from_secs_f64(request.timeout_seconds)),
    );
    let scan = workspace.scan(
        &ScanRequest {
            selections: request.patterns,
            include_hidden: request.include_hidden,
            respect_gitignore: request.respect_gitignore,
            include_node_modules: request.include_node_modules,
            file_kind,
            metadata,
            order: scan_order,
            max_files: request.max_scan_files,
        },
        &control,
    )?;
    let mut entries = scan.entries;
    if request.order == "modified_desc" {
        entries.sort_by(|left, right| {
            right
                .modified
                .cmp(&left.modified)
                .then(left.relative.cmp(&right.relative))
        });
    }
    let truncated = entries.len() > request.limit || scan.completion != WorkCompletion::Complete;
    entries.truncate(request.limit);
    let matches = entries
        .into_iter()
        .map(|entry| {
            let file_type = match entry.file_type {
                WorkspaceFileType::File => "file",
                WorkspaceFileType::Directory => "directory",
            };
            let path = if entry.file_type == WorkspaceFileType::Directory {
                format!("{}/", entry.relative)
            } else {
                entry.relative
            };
            let modified = entry
                .modified
                .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
                .map(|value| value.as_secs_f64());

            (path, file_type.to_owned(), entry.size, modified) as NativeGlobMatch
        })
        .collect();

    Ok((
        matches,
        completion(scan.completion).to_owned(),
        scan.scanned_entries,
        scan.skipped_entries,
        truncated,
    ))
}

pub(crate) fn completion(value: WorkCompletion) -> &'static str {
    match value {
        WorkCompletion::Complete => "complete",
        WorkCompletion::FileLimitReached => "file_limit_reached",
        WorkCompletion::DeadlineReached => "deadline_reached",
    }
}
