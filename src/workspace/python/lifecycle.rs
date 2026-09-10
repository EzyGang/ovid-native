use pyo3::prelude::*;

use crate::python_boundary::register_exports;
use crate::workspace::python::errors::to_python_error;
use crate::workspace::python::{NativeWorkspace, NativeWorkspaceMutation};
use crate::workspace::{
    Cancellation, MutationContext, PolicyGeneration, Workspace, WorkspaceError, WorkspacePolicy,
};

type NativePolicy = (bool, f64, u64, u64, usize, usize, bool, u64);

#[pyfunction]
fn workspace_create(root: String) -> PyResult<NativeWorkspace> {
    Workspace::new(&root)
        .map(NativeWorkspace::new)
        .map_err(to_python_error)
}

#[pyfunction]
fn workspace_close(workspace: PyRef<'_, NativeWorkspace>) {
    workspace.inner.close();
}

#[pyfunction]
fn workspace_is_closed(workspace: PyRef<'_, NativeWorkspace>) -> bool {
    workspace.inner.is_closed()
}

#[pyfunction]
fn workspace_revision(workspace: PyRef<'_, NativeWorkspace>) -> u64 {
    workspace.inner.revision()
}

#[pyfunction]
fn workspace_policy(workspace: PyRef<'_, NativeWorkspace>) -> PyResult<NativePolicy> {
    workspace
        .inner
        .policy()
        .map(policy_to_native)
        .map_err(to_python_error)
}

#[pyfunction]
fn workspace_set_policy(
    workspace: PyRef<'_, NativeWorkspace>,
    policy: (bool, f64, u64, u64, usize, usize, bool),
) -> PyResult<NativePolicy> {
    let (
        allow_fuzzy_replace,
        fuzzy_replace_threshold,
        max_read_bytes,
        max_observation_file_bytes,
        max_observation_entries,
        max_observation_store_bytes,
        create_parent_directories,
    ) = policy;
    workspace
        .inner
        .set_policy(WorkspacePolicy {
            allow_fuzzy_replace,
            fuzzy_replace_threshold,
            max_read_bytes,
            max_observation_file_bytes,
            max_observation_entries,
            max_observation_store_bytes,
            create_parent_directories,
        })
        .map(policy_to_native)
        .map_err(to_python_error)
}

#[pyfunction]
fn workspace_edit_mode(workspace: PyRef<'_, NativeWorkspace>) -> PyResult<(String, u64)> {
    workspace
        .inner
        .edit_mode()
        .map(|selection| (selection.mode, selection.generation))
        .map_err(to_python_error)
}

#[pyfunction]
fn workspace_set_edit_mode(
    workspace: PyRef<'_, NativeWorkspace>,
    mode: String,
) -> PyResult<(String, u64)> {
    workspace
        .inner
        .set_edit_mode(&mode)
        .map(|selection| (selection.mode, selection.generation))
        .map_err(to_python_error)
}

#[pyfunction]
#[pyo3(signature = (workspace, mode=None))]
fn workspace_capture_mutation(
    workspace: PyRef<'_, NativeWorkspace>,
    mode: Option<String>,
) -> PyResult<NativeWorkspaceMutation> {
    let selection = workspace.inner.edit_mode().map_err(to_python_error)?;
    let policy = workspace.inner.policy().map_err(to_python_error)?;
    let captured_mode = mode.unwrap_or(selection.mode);
    if !workspace
        .inner
        .supports_edit_mode(&captured_mode)
        .map_err(to_python_error)?
    {
        return Err(to_python_error(WorkspaceError::EditMode(format!(
            "workspace edit mode is not registered: {captured_mode}"
        ))));
    }

    Ok(NativeWorkspaceMutation {
        context: MutationContext {
            mode: captured_mode,
            mode_generation: selection.generation,
            policy_generation: policy.generation,
            cancellation: Cancellation::new(),
            policy: policy.policy,
        },
    })
}

#[pyfunction]
fn workspace_register_edit_mode(
    workspace: PyRef<'_, NativeWorkspace>,
    mode: String,
) -> PyResult<()> {
    workspace
        .inner
        .register_edit_mode(&mode)
        .map_err(to_python_error)
}

fn policy_to_native(generation: PolicyGeneration) -> NativePolicy {
    let policy = generation.policy;
    (
        policy.allow_fuzzy_replace,
        policy.fuzzy_replace_threshold,
        policy.max_read_bytes,
        policy.max_observation_file_bytes,
        policy.max_observation_entries,
        policy.max_observation_store_bytes,
        policy.create_parent_directories,
        generation.generation,
    )
}

pub(super) fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    register_exports!(module, classes [], functions [
        workspace_create, workspace_close, workspace_is_closed, workspace_revision,
        workspace_policy, workspace_set_policy, workspace_edit_mode, workspace_set_edit_mode,
        workspace_capture_mutation, workspace_register_edit_mode,
    ]);
    Ok(())
}
