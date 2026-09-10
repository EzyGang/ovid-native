use std::sync::{Arc, atomic::AtomicBool};

use pyo3::prelude::*;

use crate::fff::find::find;
use crate::fff::grep::{cancelled, grep, multi_grep};
use crate::fff::python_errors::{add_exceptions, to_python_error};
use crate::fff::python_types::{
    NativeFffConfig, NativeFffFindRequest, NativeFffGrepRequest, NativeFffLimits,
    NativeFffMultiGrepRequest,
};
use crate::fff::types::{
    NativeFffCancellation, NativeFffFindResult, NativeFffGrepResult, NativeFffIndexStatus,
};
use crate::fff::{
    FffError,
    engine::{FffEngineState, NativeFffEngine},
};
use crate::python_boundary::{register_exports, request_operation};
use crate::workspace::NativeWorkspace;

#[pyfunction]
fn fff_create(
    workspace: PyRef<'_, NativeWorkspace>,
    config: PyRef<'_, NativeFffConfig>,
    limits: PyRef<'_, NativeFffLimits>,
) -> PyResult<NativeFffEngine> {
    NativeFffEngine::new(
        workspace.inner.clone(),
        config.inner.clone(),
        limits.inner.clone(),
    )
    .map_err(to_python_error)
}

#[pyfunction]
fn fff_start(py: Python<'_>, engine: PyRef<'_, NativeFffEngine>) -> PyResult<NativeFffIndexStatus> {
    let engine = engine.inner.clone();
    py.detach(move || engine.start()).map_err(to_python_error)
}

#[pyfunction]
fn fff_wait_ready(
    py: Python<'_>,
    engine: PyRef<'_, NativeFffEngine>,
    timeout_seconds: f64,
) -> PyResult<NativeFffIndexStatus> {
    let engine = engine.inner.clone();
    py.detach(move || engine.wait_ready(timeout_seconds))
        .map_err(to_python_error)
}

#[pyfunction]
fn fff_status(engine: PyRef<'_, NativeFffEngine>) -> PyResult<NativeFffIndexStatus> {
    engine.inner.status().map_err(to_python_error)
}

#[pyfunction]
fn fff_rescan(
    py: Python<'_>,
    engine: PyRef<'_, NativeFffEngine>,
) -> PyResult<NativeFffIndexStatus> {
    let engine = engine.inner.clone();
    py.detach(move || engine.rescan()).map_err(to_python_error)
}

#[pyfunction]
fn fff_close(py: Python<'_>, engine: PyRef<'_, NativeFffEngine>) -> PyResult<()> {
    let engine = engine.inner.clone();
    py.detach(move || engine.close()).map_err(to_python_error)
}

#[pyfunction]
fn fff_find(
    py: Python<'_>,
    engine: PyRef<'_, NativeFffEngine>,
    request: PyRef<'_, NativeFffFindRequest>,
) -> PyResult<NativeFffFindResult> {
    request_operation(
        py,
        &engine.inner,
        &request.inner,
        |engine, request| find(engine, request),
        to_python_error,
    )
}

#[pyfunction]
fn fff_grep(
    py: Python<'_>,
    engine: PyRef<'_, NativeFffEngine>,
    request: PyRef<'_, NativeFffGrepRequest>,
    cancellation: PyRef<'_, NativeFffCancellation>,
) -> PyResult<NativeFffGrepResult> {
    grep_operation(py, &engine, &request.inner, &cancellation, grep)
}

#[pyfunction]
fn fff_multi_grep(
    py: Python<'_>,
    engine: PyRef<'_, NativeFffEngine>,
    request: PyRef<'_, NativeFffMultiGrepRequest>,
    cancellation: PyRef<'_, NativeFffCancellation>,
) -> PyResult<NativeFffGrepResult> {
    grep_operation(py, &engine, &request.inner, &cancellation, multi_grep)
}

#[pyfunction]
fn fff_version() -> &'static str {
    "0.10.3"
}

pub(crate) fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    register_exports!(module,
        classes [NativeFffEngine, NativeFffCancellation, NativeFffConfig, NativeFffLimits, NativeFffFindRequest, NativeFffGrepRequest, NativeFffMultiGrepRequest],
        functions [fff_create, fff_start, fff_wait_ready, fff_status, fff_rescan, fff_close, fff_find, fff_grep, fff_multi_grep, fff_version]
    );
    add_exceptions(module)
}

fn grep_operation<T>(
    py: Python<'_>,
    engine: &NativeFffEngine,
    request: &T,
    cancellation: &NativeFffCancellation,
    operation: fn(&FffEngineState, T, Arc<AtomicBool>) -> Result<NativeFffGrepResult, FffError>,
) -> PyResult<NativeFffGrepResult>
where
    T: Clone + Send,
{
    let signal = cancellation.signal();
    request_operation(
        py,
        &engine.inner,
        request,
        move |engine, request| {
            cancelled(&signal)?;
            let result = operation(engine, request, signal.clone())?;
            cancelled(&signal)?;
            Ok(result)
        },
        to_python_error,
    )
}
