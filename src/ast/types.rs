use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use pyo3::prelude::*;

pub type Position = (usize, usize, usize);
pub type Range = (Position, Position);
pub type Capture = (String, String, Option<Range>);
pub type Match = (String, String, String, Range, Vec<Capture>);
pub type Issue = (Option<String>, Option<String>, String, String);
pub type SearchResult = (Vec<Match>, usize, usize, usize, usize, bool, Vec<Issue>);
pub type Change = (String, String, String, String, Range);
pub type FileChange = (String, String, String, usize);
pub type PreviewResult = (
    NativeAstRewriteComputation,
    Vec<Change>,
    Vec<FileChange>,
    usize,
    usize,
    Vec<Issue>,
);
pub type ApplyResult = (Vec<FileChange>, usize);

#[derive(Clone)]
pub struct Cancellation {
    cancelled: Arc<AtomicBool>,
}

impl Cancellation {
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

#[derive(Clone, Copy)]
pub struct Limits {
    pub max_matches: usize,
    pub max_files: usize,
    pub max_file_bytes: usize,
    pub max_replacements: usize,
    pub max_changed_files: usize,
}

#[derive(Clone)]
pub struct ScanOptions {
    pub paths: Vec<String>,
    pub include_hidden: bool,
    pub respect_gitignore: bool,
    pub include_node_modules: bool,
}

#[derive(Clone)]
pub struct SearchRequest {
    pub pattern: String,
    pub scan: ScanOptions,
    pub language: Option<String>,
    pub strictness: String,
    pub offset: usize,
    pub limit: usize,
    pub include_captures: bool,
    pub limits: Limits,
    pub cancellation: Cancellation,
}

#[derive(Clone)]
pub struct RewriteRequest {
    pub operations: Vec<(String, String)>,
    pub scan: ScanOptions,
    pub language: Option<String>,
    pub strictness: String,
    pub limits: Limits,
    pub cancellation: Cancellation,
}

#[derive(Clone)]
pub struct FileComputation {
    pub path: String,
    pub original_sha256: String,
    pub updated_sha256: String,
    pub updated: String,
    pub replacements: usize,
}

pub struct RewriteComputation {
    pub root: PathBuf,
    pub files: Vec<FileComputation>,
    pub total_replacements: usize,
}

#[pyclass(frozen, skip_from_py_object)]
#[derive(Clone)]
pub struct NativeAstCancellation {
    inner: Cancellation,
}

#[pymethods]
impl NativeAstCancellation {
    #[new]
    pub fn new() -> Self {
        Self {
            inner: Cancellation {
                cancelled: Arc::new(AtomicBool::new(false)),
            },
        }
    }

    pub fn cancel(&self) {
        self.inner.cancelled.store(true, Ordering::Relaxed);
    }
}

impl NativeAstCancellation {
    pub fn token(&self) -> Cancellation {
        self.inner.clone()
    }
}

#[pyclass(frozen, skip_from_py_object)]
#[derive(Clone)]
pub struct NativeAstRewriteComputation {
    pub inner: Arc<RewriteComputation>,
}

impl NativeAstRewriteComputation {
    pub fn new(computation: RewriteComputation) -> Self {
        Self {
            inner: Arc::new(computation),
        }
    }
}
