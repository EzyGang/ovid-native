use std::time::Duration;

use crate::search::{
    SearchError,
    glob::completion,
    grep_file::{FileSearch, search_file},
    grep_matcher::{CompiledPattern, compile_pattern},
    types::{GrepRequest, NativeGrepFileMatches, NativeGrepResult},
};
use crate::workspace::{
    MetadataLevel, ScanFileKind, ScanOrder, ScanRequest, WorkCompletion, WorkControl, WorkStopped,
    Workspace, WorkspaceError,
};

#[derive(Default)]
struct SearchPage {
    files: Vec<NativeGrepFileMatches>,
    files_searched: usize,
    files_with_matches: usize,
    skipped_binary_files: usize,
    skipped_encoding_files: usize,
    skipped_large_files: usize,
    retained_matches: usize,
    stopped_early: bool,
}

pub(crate) fn grep(
    workspace: &Workspace,
    request: GrepRequest,
) -> Result<NativeGrepResult, SearchError> {
    validate_request(&request)?;
    let compiled = compile_pattern(&request)?;
    let control = WorkControl::new(
        request.cancellation.clone(),
        Some(Duration::from_secs_f64(request.timeout_seconds)),
    );
    let scan = workspace.scan(
        &ScanRequest {
            selections: request.paths.clone(),
            include_hidden: request.include_hidden,
            respect_gitignore: request.respect_gitignore,
            include_node_modules: request.include_node_modules,
            file_kind: ScanFileKind::Files,
            metadata: MetadataLevel::Size,
            order: ScanOrder::Path,
            max_files: request.max_scan_files,
        },
        &control,
    )?;

    search_entries(scan, &request, &compiled, &control)
}

fn search_entries(
    scan: crate::workspace::ScanResult,
    request: &GrepRequest,
    compiled: &CompiledPattern,
    control: &WorkControl,
) -> Result<NativeGrepResult, SearchError> {
    let mut page = SearchPage::default();
    let mut operation_completion = scan.completion;

    for entry in scan.entries {
        match control.checkpoint() {
            Ok(()) => (),
            Err(WorkStopped::Cancelled) => return Err(SearchError::Cancelled),
            Err(WorkStopped::Deadline) => {
                operation_completion = WorkCompletion::DeadlineReached;
                page.stopped_early = true;
                break;
            }
        }
        if entry
            .size
            .is_some_and(|size| size > request.max_file_bytes as u64)
            && request.large_file_mode == "skip"
        {
            page.skipped_large_files += 1;
            continue;
        }

        let searched = match search_file(&entry, request, compiled, control) {
            Ok(value) => value,
            Err(WorkspaceError::Cancelled) => return Err(SearchError::Cancelled),
            Err(WorkspaceError::Deadline) => {
                operation_completion = WorkCompletion::DeadlineReached;
                page.stopped_early = true;
                break;
            }
            Err(WorkspaceError::Read(message)) => {
                return Err(SearchError::Read(format!(
                    "cannot read {}: {message}",
                    entry.relative
                )));
            }
            Err(error) => return Err(error.into()),
        };
        if !page.collect(searched, request) {
            break;
        }
    }

    Ok(page.finish(request, compiled, operation_completion))
}

impl SearchPage {
    fn collect(&mut self, searched: FileSearch, request: &GrepRequest) -> bool {
        let mut file = match searched {
            FileSearch::Binary => {
                self.skipped_binary_files += 1;
                return true;
            }
            FileSearch::Encoding => {
                self.skipped_encoding_files += 1;
                return true;
            }
            FileSearch::Searched(file) => file,
        };
        self.files_searched += 1;
        if file.1.is_empty() && file.2 == 0 {
            return true;
        }
        self.files_with_matches += 1;
        if self.files_with_matches <= request.file_offset {
            return true;
        }
        if self.files.len() == request.file_limit {
            self.stopped_early = true;
            return false;
        }
        let available = request
            .max_grep_matches
            .saturating_sub(self.retained_matches);
        if file.1.len() > available {
            file.1.truncate(available);
            file.3 = true;
            self.stopped_early = true;
        }
        self.retained_matches += file.1.len();
        self.files.push(file);
        if self.retained_matches == request.max_grep_matches {
            self.stopped_early = true;
            return false;
        }
        true
    }

    fn finish(
        self,
        request: &GrepRequest,
        compiled: &CompiledPattern,
        operation_completion: WorkCompletion,
    ) -> NativeGrepResult {
        let incomplete = self.stopped_early || operation_completion != WorkCompletion::Complete;
        let truncated = incomplete || self.files.iter().any(|file| file.3 || !file.4 || !file.5.2);
        let next_file_offset = incomplete.then(|| request.file_offset + self.files.len());
        (
            self.files,
            compiled.engine.to_owned(),
            compiled.interpreted_as_literal,
            completion(operation_completion).to_owned(),
            self.files_searched,
            self.files_with_matches,
            !incomplete,
            self.skipped_binary_files,
            self.skipped_encoding_files,
            self.skipped_large_files,
            next_file_offset,
            truncated,
        )
    }
}

fn validate_request(request: &GrepRequest) -> Result<(), SearchError> {
    if request.pattern.contains('\0') {
        return Err(SearchError::Pattern(
            "grep pattern must contain no NUL bytes".to_owned(),
        ));
    }
    if !matches!(request.mode.as_str(), "regex" | "literal" | "auto") {
        return Err(SearchError::Configuration(format!(
            "invalid grep pattern mode: {}",
            request.mode
        )));
    }
    if !matches!(request.large_file_mode.as_str(), "skip" | "prefix") {
        return Err(SearchError::Configuration(format!(
            "invalid large-file mode: {}",
            request.large_file_mode
        )));
    }

    Ok(())
}
