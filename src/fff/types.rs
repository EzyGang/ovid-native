use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use pyo3::prelude::*;

pub(crate) type NativeFffIndexStatus = (String, usize, bool, bool, bool);
pub(crate) type NativeFffPathMatch = (String, String, bool, Option<u64>, Option<u64>, String);
pub(crate) type NativeFffFindResult = (Vec<NativeFffPathMatch>, usize, Option<usize>, bool);
pub(crate) type NativeFffByteRange = (usize, usize);
pub(crate) type NativeFffContextLine = (usize, String);
pub(crate) type NativeFffGrepMatch = (
    String,
    usize,
    usize,
    u64,
    String,
    Vec<NativeFffByteRange>,
    Vec<NativeFffContextLine>,
    Vec<NativeFffContextLine>,
    bool,
    bool,
    String,
);
pub(crate) type NativeFffGrepResult = (
    Vec<NativeFffGrepMatch>,
    String,
    Option<String>,
    bool,
    String,
    usize,
    usize,
    usize,
    usize,
    Option<usize>,
    bool,
);

#[derive(Clone, Debug)]
pub(crate) struct FffConfig {
    pub watch: bool,
    pub enable_content_indexing: bool,
    pub enable_mmap_cache: bool,
    pub initial_scan_timeout_seconds: f64,
    pub search_timeout_seconds: f64,
}

#[derive(Clone, Debug)]
pub(crate) struct FffLimits {
    pub max_results: usize,
    pub max_matches_per_file: usize,
    pub max_patterns: usize,
    pub max_pattern_characters: usize,
    pub max_query_characters: usize,
    pub max_file_bytes: u64,
    pub max_context_lines: usize,
    pub max_search_timeout_seconds: f64,
}

#[derive(Clone, Debug)]
pub(crate) struct FffConstraints {
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub git_status: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct FffFindRequest {
    pub query: String,
    pub constraints: FffConstraints,
    pub kind: String,
    pub offset: usize,
    pub limit: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct FffGrepRequest {
    pub query: String,
    pub constraints: FffConstraints,
    pub mode: String,
    pub smart_case: bool,
    pub file_offset: usize,
    pub limit: usize,
    pub matches_per_file: usize,
    pub context_before: usize,
    pub context_after: usize,
    pub max_file_bytes: u64,
    pub timeout_seconds: f64,
    pub classify_definitions: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct FffMultiGrepRequest {
    pub patterns: Vec<String>,
    pub constraints: FffConstraints,
    pub smart_case: bool,
    pub file_offset: usize,
    pub limit: usize,
    pub matches_per_file: usize,
    pub context_before: usize,
    pub context_after: usize,
    pub max_file_bytes: u64,
    pub timeout_seconds: f64,
    pub classify_definitions: bool,
}

#[pyclass(frozen, skip_from_py_object)]
#[derive(Clone, Debug)]
pub(crate) struct NativeFffCancellation {
    inner: Arc<AtomicBool>,
}

#[pymethods]
impl NativeFffCancellation {
    #[new]
    fn new() -> Self {
        Self {
            inner: Arc::new(AtomicBool::new(false)),
        }
    }

    fn cancel(&self) {
        self.inner.store(true, Ordering::Release);
    }
}

impl NativeFffCancellation {
    pub(crate) fn signal(&self) -> Arc<AtomicBool> {
        self.inner.clone()
    }
}
