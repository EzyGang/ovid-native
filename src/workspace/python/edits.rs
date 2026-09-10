use pyo3::prelude::*;

use crate::python_boundary::register_exports;
use crate::workspace::python::results::NativeEditResult;
use crate::workspace::python::{
    NativeWorkspace, NativeWorkspaceCancellation, NativeWorkspaceMutation,
};
use crate::workspace::{
    HashlineOperation, HashlineSection, parse_apply_patch, parse_structured_patch,
};

type NativeHashlineOperation = (
    String,
    Option<usize>,
    Option<String>,
    Option<usize>,
    Option<String>,
    Vec<String>,
    Option<String>,
    Option<String>,
);
type NativeHashlineSection = (String, String, Vec<NativeHashlineOperation>);

#[pyfunction]
fn workspace_replace_text(
    py: Python<'_>,
    workspace: PyRef<'_, NativeWorkspace>,
    mutation: PyRef<'_, NativeWorkspaceMutation>,
    path: String,
    replacement: (String, String, bool),
    cancellation: PyRef<'_, NativeWorkspaceCancellation>,
) -> PyResult<NativeEditResult> {
    workspace.mutate(py, &mutation, &cancellation, move |workspace, context| {
        workspace.replace_text(
            &path,
            &replacement.0,
            &replacement.1,
            replacement.2,
            &context,
        )
    })
}

#[pyfunction]
fn workspace_patch(
    py: Python<'_>,
    workspace: PyRef<'_, NativeWorkspace>,
    mutation: PyRef<'_, NativeWorkspaceMutation>,
    path: String,
    edits: Vec<(String, Option<String>, Option<String>)>,
    cancellation: PyRef<'_, NativeWorkspaceCancellation>,
) -> PyResult<NativeEditResult> {
    workspace.mutate(py, &mutation, &cancellation, move |workspace, context| {
        let operations = parse_structured_patch(&path, &edits)?;
        workspace.apply_patch_operations(&operations, &context, "patch")
    })
}

#[pyfunction]
fn workspace_apply_patch(
    py: Python<'_>,
    workspace: PyRef<'_, NativeWorkspace>,
    mutation: PyRef<'_, NativeWorkspaceMutation>,
    input: String,
    cancellation: PyRef<'_, NativeWorkspaceCancellation>,
) -> PyResult<NativeEditResult> {
    workspace.mutate(py, &mutation, &cancellation, move |workspace, context| {
        let operations = parse_apply_patch(&input)?;
        workspace.apply_patch_operations(&operations, &context, "apply_patch")
    })
}

#[pyfunction]
fn workspace_hashline(
    py: Python<'_>,
    workspace: PyRef<'_, NativeWorkspace>,
    mutation: PyRef<'_, NativeWorkspaceMutation>,
    sections: Vec<NativeHashlineSection>,
    cancellation: PyRef<'_, NativeWorkspaceCancellation>,
) -> PyResult<NativeEditResult> {
    let sections = sections
        .into_iter()
        .map(|(path, tag, operations)| HashlineSection {
            path,
            tag,
            operations: operations
                .into_iter()
                .map(
                    |(kind, start, start_hash, end, end_hash, body, register, destination)| {
                        HashlineOperation {
                            kind,
                            start,
                            start_hash,
                            end,
                            end_hash,
                            body,
                            register,
                            destination,
                        }
                    },
                )
                .collect(),
        })
        .collect::<Vec<_>>();
    workspace.mutate(py, &mutation, &cancellation, move |workspace, context| {
        workspace.apply_hashline(&sections, &context)
    })
}

pub(super) fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    register_exports!(module, classes [], functions [
        workspace_replace_text, workspace_patch, workspace_apply_patch, workspace_hashline,
    ]);
    Ok(())
}
