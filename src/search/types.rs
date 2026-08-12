use std::sync::Arc;

use pyo3::prelude::*;

use crate::workspace::Cancellation;

pub(crate) type NativeGlobMatch = (String, String, Option<u64>, Option<f64>);
pub(crate) type NativeGlobResult = (Vec<NativeGlobMatch>, String, usize, usize, bool);
pub(crate) type NativeGrepPosition = (usize, usize, usize);
pub(crate) type NativeGrepRange = (NativeGrepPosition, NativeGrepPosition);
pub(crate) type NativeGrepContextLine = (usize, String, bool);
pub(crate) type NativeGrepMatch = (
    String,
    NativeGrepRange,
    String,
    bool,
    Vec<NativeGrepContextLine>,
    Vec<NativeGrepContextLine>,
);
pub(crate) type NativeGrepCoverage = (u64, u64, bool);
pub(crate) type NativeGrepFileMatches = (
    String,
    Vec<NativeGrepMatch>,
    usize,
    bool,
    bool,
    NativeGrepCoverage,
);
pub(crate) type NativeGrepResult = (
    Vec<NativeGrepFileMatches>,
    String,
    bool,
    String,
    usize,
    usize,
    bool,
    usize,
    usize,
    usize,
    Option<usize>,
    bool,
);

#[derive(Clone, Debug)]
pub(crate) struct GlobRequest {
    pub patterns: Vec<String>,
    pub include_hidden: bool,
    pub respect_gitignore: bool,
    pub include_node_modules: bool,
    pub file_type: String,
    pub order: String,
    pub limit: usize,
    pub max_scan_files: usize,
    pub timeout_seconds: f64,
    pub cancellation: Cancellation,
}

#[derive(Clone, Debug)]
pub(crate) struct GrepRequest {
    pub pattern: String,
    pub paths: Vec<String>,
    pub include_hidden: bool,
    pub respect_gitignore: bool,
    pub include_node_modules: bool,
    pub mode: String,
    pub case_sensitive: bool,
    pub multiline: bool,
    pub file_offset: usize,
    pub file_limit: usize,
    pub matches_per_file: usize,
    pub context_before: usize,
    pub context_after: usize,
    pub max_file_bytes: usize,
    pub large_file_mode: String,
    pub timeout_seconds: f64,
    pub max_scan_files: usize,
    pub max_grep_matches: usize,
    pub max_matches_per_file: usize,
    pub max_line_characters: usize,
    pub cancellation: Cancellation,
}

#[pyclass(frozen, skip_from_py_object)]
#[derive(Clone)]
pub(crate) struct NativeSearchCancellation {
    inner: Cancellation,
}

#[pymethods]
impl NativeSearchCancellation {
    #[new]
    pub(crate) fn new() -> Self {
        Self {
            inner: Cancellation::new(),
        }
    }

    pub(crate) fn cancel(&self) {
        self.inner.cancel();
    }
}

impl NativeSearchCancellation {
    pub(crate) fn token(&self) -> Cancellation {
        self.inner.clone()
    }
}

#[pyclass(frozen, skip_from_py_object)]
#[derive(Clone)]
pub(crate) struct NativeWorkspace {
    pub(crate) inner: Arc<crate::workspace::Workspace>,
}

impl NativeWorkspace {
    pub(crate) fn new(workspace: crate::workspace::Workspace) -> Self {
        Self {
            inner: Arc::new(workspace),
        }
    }
}

#[pymethods]
impl NativeWorkspace {
    #[getter]
    pub(crate) fn root(&self) -> String {
        self.inner.root().to_string_lossy().into_owned()
    }
}
