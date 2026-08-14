use pyo3::create_exception;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;

use crate::ast::AstError;
use crate::ast::language::{LanguageInfo, supported_languages};
use crate::ast::rewrite::{apply, preview};
use crate::ast::search::search;
use crate::ast::types::{
    ApplyResult, Limits, NativeAstCancellation, NativeAstRewriteComputation, PreviewResult,
    RewriteRequest, ScanOptions, SearchRequest, SearchResult,
};
use crate::workspace::{NativeWorkspace, closed_python_error};

create_exception!(_native, NativeAstConfigurationError, PyException);
create_exception!(_native, NativeAstPathError, PyException);
create_exception!(_native, NativeAstLanguageError, PyException);
create_exception!(_native, NativeAstPatternError, PyException);
create_exception!(_native, NativeAstLimitError, PyException);
create_exception!(_native, NativeAstProposalStaleError, PyException);
create_exception!(_native, NativeAstWriteError, PyException);
create_exception!(_native, NativeAstCancelledError, PyException);

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
    let operation = workspace.inner.begin().ok_or_else(closed_python_error)?;
    let workspace = workspace.inner.clone();
    let mut request = request.inner.clone();
    request.cancellation = request.cancellation.with_parent(workspace.cancellation());
    py.detach(move || {
        let _operation = operation;
        search(&workspace, request)
    })
    .map_err(to_python_error)
}

#[pyfunction]
fn ast_preview_rewrite(
    py: Python<'_>,
    workspace: PyRef<'_, NativeWorkspace>,
    request: PyRef<'_, NativeAstRewriteRequest>,
) -> PyResult<PreviewResult> {
    let operation = workspace.inner.begin().ok_or_else(closed_python_error)?;
    let workspace = workspace.inner.clone();
    let mut request = request.inner.clone();
    request.cancellation = request.cancellation.with_parent(workspace.cancellation());
    py.detach(move || {
        let _operation = operation;
        preview(&workspace, request)
    })
    .map_err(to_python_error)
}

#[pyfunction]
fn ast_apply_rewrite(
    py: Python<'_>,
    workspace: PyRef<'_, NativeWorkspace>,
    computation: PyRef<'_, NativeAstRewriteComputation>,
    cancellation: PyRef<'_, NativeAstCancellation>,
) -> PyResult<ApplyResult> {
    let operation = workspace.inner.begin().ok_or_else(closed_python_error)?;
    let workspace = workspace.inner.clone();
    let computation = computation.inner.clone();
    let cancellation = cancellation.token().with_parent(workspace.cancellation());
    py.detach(move || {
        let _operation = operation;
        apply(&workspace, computation, &cancellation)
    })
    .map_err(to_python_error)
}

pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<NativeAstCancellation>()?;
    module.add_class::<NativeAstLimits>()?;
    module.add_class::<NativeAstScanOptions>()?;
    module.add_class::<NativeAstSearchRequest>()?;
    module.add_class::<NativeAstRewriteRequest>()?;
    module.add_class::<NativeAstRewriteComputation>()?;
    module.add_function(wrap_pyfunction!(ast_supported_languages, module)?)?;
    module.add_function(wrap_pyfunction!(ast_grep_version, module)?)?;
    module.add_function(wrap_pyfunction!(ast_search, module)?)?;
    module.add_function(wrap_pyfunction!(ast_preview_rewrite, module)?)?;
    module.add_function(wrap_pyfunction!(ast_apply_rewrite, module)?)?;
    add_exceptions(module)?;
    Ok(())
}

fn add_exceptions(module: &Bound<'_, PyModule>) -> PyResult<()> {
    let py = module.py();
    module.add(
        "NativeAstConfigurationError",
        py.get_type::<NativeAstConfigurationError>(),
    )?;
    module.add("NativeAstPathError", py.get_type::<NativeAstPathError>())?;
    module.add(
        "NativeAstLanguageError",
        py.get_type::<NativeAstLanguageError>(),
    )?;
    module.add(
        "NativeAstPatternError",
        py.get_type::<NativeAstPatternError>(),
    )?;
    module.add("NativeAstLimitError", py.get_type::<NativeAstLimitError>())?;
    module.add(
        "NativeAstProposalStaleError",
        py.get_type::<NativeAstProposalStaleError>(),
    )?;
    module.add("NativeAstWriteError", py.get_type::<NativeAstWriteError>())?;
    module.add(
        "NativeAstCancelledError",
        py.get_type::<NativeAstCancelledError>(),
    )?;
    Ok(())
}

fn to_python_error(error: AstError) -> PyErr {
    match error {
        AstError::Configuration(message) => NativeAstConfigurationError::new_err(message),
        AstError::Path(message) => NativeAstPathError::new_err(message),
        AstError::Language(message) => NativeAstLanguageError::new_err(message),
        AstError::Pattern(message) => NativeAstPatternError::new_err(message),
        AstError::Limit(message) => NativeAstLimitError::new_err(message),
        AstError::Stale(message) => NativeAstProposalStaleError::new_err(message),
        AstError::Write(message) => NativeAstWriteError::new_err(message),
        AstError::Cancelled => NativeAstCancelledError::new_err("AST operation cancelled"),
    }
}
