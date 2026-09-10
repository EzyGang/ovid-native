use pyo3::prelude::*;

use crate::python_boundary::register_exports;
use crate::workspace::python::results::NativeEditResult;
use crate::workspace::python::{NativeWorkspace, NativeWorkspaceCancellation};

#[pyfunction]
fn workspace_create_file(
    py: Python<'_>,
    workspace: PyRef<'_, NativeWorkspace>,
    path: String,
    content: String,
    create_parents: bool,
    cancellation: PyRef<'_, NativeWorkspaceCancellation>,
) -> PyResult<NativeEditResult> {
    workspace.write(py, &cancellation, move |workspace, context| {
        workspace.create_text_file(&path, &content, create_parents, &context)
    })
}

#[pyfunction]
fn workspace_replace_file(
    py: Python<'_>,
    workspace: PyRef<'_, NativeWorkspace>,
    path: String,
    content: String,
    expected_observation: String,
    cancellation: PyRef<'_, NativeWorkspaceCancellation>,
) -> PyResult<NativeEditResult> {
    workspace.write(py, &cancellation, move |workspace, context| {
        workspace.replace_text_file(&path, &content, &expected_observation, &context)
    })
}

#[pyfunction]
fn workspace_delete_file(
    py: Python<'_>,
    workspace: PyRef<'_, NativeWorkspace>,
    path: String,
    cancellation: PyRef<'_, NativeWorkspaceCancellation>,
) -> PyResult<NativeEditResult> {
    workspace.write(py, &cancellation, move |workspace, context| {
        workspace.delete_text_file(&path, &context)
    })
}

#[pyfunction]
fn workspace_move_file(
    py: Python<'_>,
    workspace: PyRef<'_, NativeWorkspace>,
    path: String,
    destination: String,
    cancellation: PyRef<'_, NativeWorkspaceCancellation>,
) -> PyResult<NativeEditResult> {
    workspace.write(py, &cancellation, move |workspace, context| {
        workspace.move_text_file(&path, &destination, &context)
    })
}

pub(super) fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    register_exports!(module, classes [], functions [
        workspace_create_file, workspace_replace_file, workspace_delete_file, workspace_move_file,
    ]);
    Ok(())
}
