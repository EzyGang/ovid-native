use pyo3::prelude::*;

use crate::python_boundary::register_exports;
use crate::workspace::LineRange;
use crate::workspace::python::NativeWorkspace;
use crate::workspace::python::results::{
    NativeDirectoryRead, NativeDirectoryTreeRead, NativeFileRead, NativeObservationReceipt,
    NativeObservedSource, native_items,
};

#[pyfunction]
fn workspace_read_file(
    py: Python<'_>,
    workspace: PyRef<'_, NativeWorkspace>,
    path: String,
    ranges: Vec<(usize, Option<usize>)>,
) -> PyResult<NativeFileRead> {
    workspace.run(py, move |workspace| {
        let ranges = ranges
            .into_iter()
            .map(|(start, end)| LineRange {
                start,
                end: end.unwrap_or(usize::MAX),
            })
            .collect::<Vec<_>>();
        workspace.read_file(&path, &ranges)
    })
}

#[pyfunction]
fn workspace_list_directory(
    py: Python<'_>,
    workspace: PyRef<'_, NativeWorkspace>,
    path: String,
    depth: usize,
) -> PyResult<NativeDirectoryRead> {
    workspace.run(py, move |workspace| workspace.list_directory(&path, depth))
}

#[pyfunction]
fn workspace_read_directory_tree(
    py: Python<'_>,
    workspace: PyRef<'_, NativeWorkspace>,
    path: String,
    depth: usize,
    child_limit: usize,
) -> PyResult<NativeDirectoryTreeRead> {
    workspace.run(py, move |workspace| {
        workspace.read_directory_tree(&path, depth, child_limit)
    })
}

#[pyfunction]
fn workspace_resolve_observation(
    py: Python<'_>,
    workspace: PyRef<'_, NativeWorkspace>,
    path: String,
    tag: String,
) -> PyResult<NativeObservationReceipt> {
    workspace.run(py, move |workspace| {
        workspace.resolve_observation(&path, &tag)
    })
}

#[pyfunction]
fn workspace_validate_observed_lines(
    py: Python<'_>,
    workspace: PyRef<'_, NativeWorkspace>,
    path: String,
    tag: String,
    lines: Vec<usize>,
) -> PyResult<NativeObservationReceipt> {
    workspace.run(py, move |workspace| {
        workspace.validate_observed_lines(&path, &tag, &lines)
    })
}

#[pyfunction]
fn workspace_observe_source_lines(
    py: Python<'_>,
    workspace: PyRef<'_, NativeWorkspace>,
    path: String,
    claims: Vec<(usize, String)>,
    spans: Vec<(usize, usize, usize, usize)>,
    complete_presentation: bool,
) -> PyResult<NativeObservedSource> {
    workspace.run(py, move |workspace| {
        workspace
            .observe_source_lines(&path, &claims, &spans, complete_presentation)
            .map(|(receipt, lines)| -> NativeObservedSource {
                (receipt.into(), native_items(lines))
            })
    })
}

pub(super) fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    register_exports!(module, classes [], functions [
        workspace_read_file, workspace_list_directory, workspace_read_directory_tree,
        workspace_resolve_observation, workspace_validate_observed_lines, workspace_observe_source_lines,
    ]);
    Ok(())
}
