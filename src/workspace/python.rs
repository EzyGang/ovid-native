use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

use crate::workspace::{Workspace, WorkspaceError};

#[pyclass(frozen, skip_from_py_object)]
#[derive(Clone, Debug)]
pub(crate) struct NativeWorkspace {
    pub(crate) inner: Workspace,
}

impl NativeWorkspace {
    fn new(workspace: Workspace) -> Self {
        Self { inner: workspace }
    }
}

#[pymethods]
impl NativeWorkspace {
    #[getter]
    fn root(&self) -> String {
        self.inner.root().to_string_lossy().into_owned()
    }
}

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

pub(crate) fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<NativeWorkspace>()?;
    module.add_function(wrap_pyfunction!(workspace_create, module)?)?;
    module.add_function(wrap_pyfunction!(workspace_close, module)?)?;
    module.add_function(wrap_pyfunction!(workspace_is_closed, module)?)?;
    module.add_function(wrap_pyfunction!(workspace_revision, module)?)?;
    Ok(())
}

fn to_python_error(error: WorkspaceError) -> PyErr {
    match error {
        WorkspaceError::Configuration(message) | WorkspaceError::Path(message) => {
            PyValueError::new_err(message)
        }
        WorkspaceError::Read(message)
        | WorkspaceError::Stale(message)
        | WorkspaceError::Write(message) => PyValueError::new_err(message),
        WorkspaceError::Cancelled => PyValueError::new_err("workspace construction was cancelled"),
        WorkspaceError::Closed => PyValueError::new_err("workspace is closed"),
        WorkspaceError::Deadline => {
            PyValueError::new_err("workspace construction reached its deadline")
        }
    }
}
