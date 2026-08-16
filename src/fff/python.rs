use pyo3::prelude::*;

use crate::fff::engine::NativeFffEngine;
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
    let engine = engine.inner.clone();
    let request = request.inner.clone();
    py.detach(move || find(&engine, request))
        .map_err(to_python_error)
}

#[pyfunction]
fn fff_grep(
    py: Python<'_>,
    engine: PyRef<'_, NativeFffEngine>,
    request: PyRef<'_, NativeFffGrepRequest>,
    cancellation: PyRef<'_, NativeFffCancellation>,
) -> PyResult<NativeFffGrepResult> {
    let engine = engine.inner.clone();
    let request = request.inner.clone();
    let signal = cancellation.signal();
    py.detach(move || {
        cancelled(&signal)?;
        let result = grep(&engine, request, signal.clone())?;
        cancelled(&signal)?;
        Ok(result)
    })
    .map_err(to_python_error)
}

#[pyfunction]
fn fff_multi_grep(
    py: Python<'_>,
    engine: PyRef<'_, NativeFffEngine>,
    request: PyRef<'_, NativeFffMultiGrepRequest>,
    cancellation: PyRef<'_, NativeFffCancellation>,
) -> PyResult<NativeFffGrepResult> {
    let engine = engine.inner.clone();
    let request = request.inner.clone();
    let signal = cancellation.signal();
    py.detach(move || {
        cancelled(&signal)?;
        let result = multi_grep(&engine, request, signal.clone())?;
        cancelled(&signal)?;
        Ok(result)
    })
    .map_err(to_python_error)
}

#[pyfunction]
fn fff_version() -> &'static str {
    "0.10.3"
}

pub(crate) fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<NativeFffEngine>()?;
    module.add_class::<NativeFffCancellation>()?;
    module.add_class::<NativeFffConfig>()?;
    module.add_class::<NativeFffLimits>()?;
    module.add_class::<NativeFffFindRequest>()?;
    module.add_class::<NativeFffGrepRequest>()?;
    module.add_class::<NativeFffMultiGrepRequest>()?;
    module.add_function(wrap_pyfunction!(fff_create, module)?)?;
    module.add_function(wrap_pyfunction!(fff_start, module)?)?;
    module.add_function(wrap_pyfunction!(fff_wait_ready, module)?)?;
    module.add_function(wrap_pyfunction!(fff_status, module)?)?;
    module.add_function(wrap_pyfunction!(fff_rescan, module)?)?;
    module.add_function(wrap_pyfunction!(fff_close, module)?)?;
    module.add_function(wrap_pyfunction!(fff_find, module)?)?;
    module.add_function(wrap_pyfunction!(fff_grep, module)?)?;
    module.add_function(wrap_pyfunction!(fff_multi_grep, module)?)?;
    module.add_function(wrap_pyfunction!(fff_version, module)?)?;
    add_exceptions(module)?;

    Ok(())
}
