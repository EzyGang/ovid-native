use pyo3::prelude::*;

use crate::python_boundary::{python_errors, register_exports, request_operation};
use crate::search::SearchError;
use crate::search::glob::glob;
use crate::search::grep::grep;
use crate::search::types::{
    GlobRequest, GrepRequest, NativeGlobResult, NativeGrepResult, NativeSearchCancellation,
};
use crate::workspace::NativeWorkspace;

python_errors! {
    SearchError;
    NativeSearchConfigurationError: SearchError::Configuration(message) => message,
    NativeSearchPathError: SearchError::Path(message) => message,
    NativeSearchPatternError: SearchError::Pattern(message) => message,
    NativeSearchLimitError: SearchError::Limit(message) => message,
    NativeSearchCancelledError: SearchError::Cancelled => "search operation cancelled",
    NativeSearchReadError: SearchError::Read(message) => message,
}

#[pyclass(frozen)]
struct NativeGlobRequest {
    inner: GlobRequest,
}

#[pymethods]
impl NativeGlobRequest {
    #[new]
    fn new(
        patterns: Vec<String>,
        scan_flags: (bool, bool, bool),
        options: (String, String, usize, usize, f64),
        cancellation: PyRef<'_, NativeSearchCancellation>,
    ) -> Self {
        Self {
            inner: GlobRequest {
                patterns,
                include_hidden: scan_flags.0,
                respect_gitignore: scan_flags.1,
                include_node_modules: scan_flags.2,
                file_type: options.0,
                order: options.1,
                limit: options.2,
                max_scan_files: options.3,
                timeout_seconds: options.4,
                cancellation: cancellation.token(),
            },
        }
    }
}

#[pyclass(frozen)]
struct NativeGrepRequest {
    inner: GrepRequest,
}

#[pymethods]
impl NativeGrepRequest {
    #[new]
    fn new(
        pattern: String,
        scan: (Vec<String>, bool, bool, bool),
        matching: (String, bool, bool),
        pagination: (usize, usize, usize),
        content: (usize, usize, usize, String, f64),
        limits: (usize, usize, usize, usize),
        cancellation: PyRef<'_, NativeSearchCancellation>,
    ) -> Self {
        Self {
            inner: GrepRequest {
                pattern,
                paths: scan.0,
                include_hidden: scan.1,
                respect_gitignore: scan.2,
                include_node_modules: scan.3,
                mode: matching.0,
                case_sensitive: matching.1,
                multiline: matching.2,
                file_offset: pagination.0,
                file_limit: pagination.1,
                matches_per_file: pagination.2,
                context_before: content.0,
                context_after: content.1,
                max_file_bytes: content.2,
                large_file_mode: content.3,
                timeout_seconds: content.4,
                max_scan_files: limits.0,
                max_grep_matches: limits.1,
                max_matches_per_file: limits.2,
                max_line_characters: limits.3,
                cancellation: cancellation.token(),
            },
        }
    }
}

#[pyfunction]
fn search_glob(
    py: Python<'_>,
    workspace: PyRef<'_, NativeWorkspace>,
    request: PyRef<'_, NativeGlobRequest>,
) -> PyResult<NativeGlobResult> {
    request_operation(py, &workspace.inner, &request.inner, glob, to_python_error)
}

#[pyfunction]
fn search_grep(
    py: Python<'_>,
    workspace: PyRef<'_, NativeWorkspace>,
    request: PyRef<'_, NativeGrepRequest>,
) -> PyResult<NativeGrepResult> {
    request_operation(py, &workspace.inner, &request.inner, grep, to_python_error)
}

pub(crate) fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    register_exports!(module,
        classes [NativeSearchCancellation, NativeGlobRequest, NativeGrepRequest],
        functions [search_glob, search_grep]
    );
    add_exceptions(module)
}
