use pyo3::prelude::*;

use crate::fff::types::{
    FffConfig, FffConstraints, FffFindRequest, FffGrepRequest, FffLimits, FffMultiGrepRequest,
};

#[pyclass(frozen)]
pub(crate) struct NativeFffConfig {
    pub(crate) inner: FffConfig,
}

#[pymethods]
impl NativeFffConfig {
    #[new]
    fn new(
        watch: bool,
        enable_content_indexing: bool,
        enable_mmap_cache: bool,
        initial_scan_timeout_seconds: f64,
        search_timeout_seconds: f64,
    ) -> Self {
        Self {
            inner: FffConfig {
                watch,
                enable_content_indexing,
                enable_mmap_cache,
                initial_scan_timeout_seconds,
                search_timeout_seconds,
            },
        }
    }
}

#[pyclass(frozen)]
pub(crate) struct NativeFffLimits {
    pub(crate) inner: FffLimits,
}

#[pymethods]
impl NativeFffLimits {
    #[new]
    #[allow(clippy::too_many_arguments)]
    fn new(
        max_results: usize,
        max_matches_per_file: usize,
        max_patterns: usize,
        max_pattern_characters: usize,
        max_query_characters: usize,
        max_file_bytes: u64,
        max_context_lines: usize,
        max_search_timeout_seconds: f64,
    ) -> Self {
        Self {
            inner: FffLimits {
                max_results,
                max_matches_per_file,
                max_patterns,
                max_pattern_characters,
                max_query_characters,
                max_file_bytes,
                max_context_lines,
                max_search_timeout_seconds,
            },
        }
    }
}

#[pyclass(frozen)]
pub(crate) struct NativeFffFindRequest {
    pub(crate) inner: FffFindRequest,
}

#[pymethods]
impl NativeFffFindRequest {
    #[new]
    fn new(
        query: String,
        constraints: (Vec<String>, Vec<String>, Option<String>),
        kind: String,
        offset: usize,
        limit: usize,
    ) -> Self {
        Self {
            inner: FffFindRequest {
                query,
                constraints: map_constraints(constraints),
                kind,
                offset,
                limit,
            },
        }
    }
}

#[pyclass(frozen)]
pub(crate) struct NativeFffGrepRequest {
    pub(crate) inner: FffGrepRequest,
}

#[pymethods]
impl NativeFffGrepRequest {
    #[new]
    fn new(
        query: String,
        constraints: (Vec<String>, Vec<String>, Option<String>),
        matching: (String, bool),
        pagination: (usize, usize, usize),
        content: (usize, usize, u64, f64, bool),
    ) -> Self {
        Self {
            inner: FffGrepRequest {
                query,
                constraints: map_constraints(constraints),
                mode: matching.0,
                smart_case: matching.1,
                file_offset: pagination.0,
                limit: pagination.1,
                matches_per_file: pagination.2,
                context_before: content.0,
                context_after: content.1,
                max_file_bytes: content.2,
                timeout_seconds: content.3,
                classify_definitions: content.4,
            },
        }
    }
}

#[pyclass(frozen)]
pub(crate) struct NativeFffMultiGrepRequest {
    pub(crate) inner: FffMultiGrepRequest,
}

#[pymethods]
impl NativeFffMultiGrepRequest {
    #[new]
    fn new(
        patterns: Vec<String>,
        constraints: (Vec<String>, Vec<String>, Option<String>),
        smart_case: bool,
        pagination: (usize, usize, usize),
        content: (usize, usize, u64, f64, bool),
    ) -> Self {
        Self {
            inner: FffMultiGrepRequest {
                patterns,
                constraints: map_constraints(constraints),
                smart_case,
                file_offset: pagination.0,
                limit: pagination.1,
                matches_per_file: pagination.2,
                context_before: content.0,
                context_after: content.1,
                max_file_bytes: content.2,
                timeout_seconds: content.3,
                classify_definitions: content.4,
            },
        }
    }
}

fn map_constraints(value: (Vec<String>, Vec<String>, Option<String>)) -> FffConstraints {
    FffConstraints {
        include: value.0,
        exclude: value.1,
        git_status: value.2,
    }
}
