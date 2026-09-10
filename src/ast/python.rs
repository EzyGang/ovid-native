use pyo3::prelude::*;

use crate::ast::AstError;
use crate::ast::language::{LanguageInfo, supported_languages};
use crate::ast::rewrite::{apply, preview};
use crate::ast::search::search;
use crate::ast::types::{
    ApplyResult, Limits, NativeAstCancellation, NativeAstRewriteComputation, PreviewResult,
    RewriteRequest, ScanOptions, SearchRequest, SearchResult,
};
use crate::python_boundary::{python_errors, register_exports};
use crate::workspace::NativeWorkspace;

python_errors! {
    AstError;
    NativeAstConfigurationError: AstError::Configuration(message) => message,
    NativeAstPathError: AstError::Path(message) => message,
    NativeAstLanguageError: AstError::Language(message) => message,
    NativeAstPatternError: AstError::Pattern(message) => message,
    NativeAstLimitError: AstError::Limit(message) => message,
    NativeAstProposalStaleError: AstError::Stale(message) => message,
    NativeAstWriteError: AstError::Write(message) => message,
    NativeAstCancelledError: AstError::Cancelled => "AST operation cancelled",
}

#[pyclass(frozen, skip_from_py_object)]
#[derive(Clone)]
struct NativeAstLimits {
    inner: Limits,
}

#[pymethods]
impl NativeAstLimits {
    #[new]
    fn new(
        max_matches: usize,
        max_files: usize,
        max_file_bytes: usize,
        max_replacements: usize,
        max_changed_files: usize,
    ) -> Self {
        Self {
            inner: Limits {
                max_matches,
                max_files,
                max_file_bytes,
                max_replacements,
                max_changed_files,
            },
        }
    }
}

#[pyclass(frozen, skip_from_py_object)]
#[derive(Clone)]
struct NativeAstScanOptions {
    inner: ScanOptions,
}

#[pymethods]
impl NativeAstScanOptions {
    #[new]
    fn new(
        paths: Vec<String>,
        include_hidden: bool,
        respect_gitignore: bool,
        include_node_modules: bool,
    ) -> Self {
        Self {
            inner: ScanOptions {
                paths,
                include_hidden,
                respect_gitignore,
                include_node_modules,
            },
        }
    }
}

#[pyclass(frozen)]
struct NativeAstSearchRequest {
    inner: SearchRequest,
}

#[pymethods]
impl NativeAstSearchRequest {
    #[new]
    fn new(
        pattern: String,
        scan: PyRef<'_, NativeAstScanOptions>,
        language: Option<String>,
        strictness: String,
        options: (usize, usize, bool),
        limits: PyRef<'_, NativeAstLimits>,
        cancellation: PyRef<'_, NativeAstCancellation>,
    ) -> Self {
        Self {
            inner: SearchRequest {
                pattern,
                scan: scan.inner.clone(),
                language,
                strictness,
                offset: options.0,
                limit: options.1,
                include_captures: options.2,
                limits: limits.inner,
                cancellation: cancellation.token(),
            },
        }
    }
}

#[pyclass(frozen)]
struct NativeAstRewriteRequest {
    inner: RewriteRequest,
}

#[pymethods]
impl NativeAstRewriteRequest {
    #[new]
    fn new(
        operations: Vec<(String, String)>,
        scan: PyRef<'_, NativeAstScanOptions>,
        language: Option<String>,
        strictness: String,
        limits: PyRef<'_, NativeAstLimits>,
        cancellation: PyRef<'_, NativeAstCancellation>,
    ) -> Self {
        Self {
            inner: RewriteRequest {
                operations,
                scan: scan.inner.clone(),
                language,
                strictness,
                limits: limits.inner,
                cancellation: cancellation.token(),
            },
        }
    }
}

#[pyfunction]
fn ast_supported_languages() -> Vec<LanguageInfo> {
    supported_languages()
}

#[pyfunction]
fn ast_grep_version() -> &'static str {
    "0.45.1"
}

#[pyfunction]
fn ast_search(
    py: Python<'_>,
    workspace: PyRef<'_, NativeWorkspace>,
    request: PyRef<'_, NativeAstSearchRequest>,
) -> PyResult<SearchResult> {
    read_operation(py, &workspace.inner, &request.inner, search)
}

#[pyfunction]
fn ast_preview_rewrite(
    py: Python<'_>,
    workspace: PyRef<'_, NativeWorkspace>,
    request: PyRef<'_, NativeAstRewriteRequest>,
) -> PyResult<PreviewResult> {
    read_operation(py, &workspace.inner, &request.inner, preview)
}

#[pyfunction]
fn ast_apply_rewrite(
    py: Python<'_>,
    workspace: PyRef<'_, NativeWorkspace>,
    computation: PyRef<'_, NativeAstRewriteComputation>,
    cancellation: PyRef<'_, NativeAstCancellation>,
) -> PyResult<ApplyResult> {
    let workspace = workspace.inner.clone();
    let computation = computation.inner.clone();
    let cancellation = cancellation.token();
    py.detach(move || {
        workspace.ensure_open().map_err(AstError::from)?;
        let _coordinator = workspace.write_guard().map_err(AstError::from)?;
        let result = apply(workspace.root(), computation, &cancellation)?;
        for file in &result.0 {
            workspace
                .mark_file_changed(&file.0)
                .map_err(AstError::from)?;
        }
        Ok(result)
    })
    .map_err(to_python_error)
}

pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    register_exports!(module,
        classes [NativeAstCancellation, NativeAstLimits, NativeAstScanOptions, NativeAstSearchRequest, NativeAstRewriteRequest, NativeAstRewriteComputation],
        functions [ast_supported_languages, ast_grep_version, ast_search, ast_preview_rewrite, ast_apply_rewrite]
    );
    add_exceptions(module)
}

fn read_operation<T, R>(
    py: Python<'_>,
    workspace: &crate::workspace::Workspace,
    request: &T,
    operation: fn(&std::path::Path, T) -> Result<R, AstError>,
) -> PyResult<R>
where
    T: Clone + Send,
    R: Send,
{
    crate::python_boundary::request_operation(
        py,
        workspace,
        request,
        move |workspace, request| {
            workspace.ensure_open().map_err(AstError::from)?;
            operation(workspace.root(), request)
        },
        to_python_error,
    )
}
