mod edits;
mod errors;
mod lifecycle;
mod reads;
mod results;
mod writes;

use pyo3::prelude::*;

use crate::python_boundary::register_exports;
use crate::workspace::python::errors::to_python_error;
use crate::workspace::python::results::NativeEditResult;
use crate::workspace::{Cancellation, EditResult, MutationContext, Workspace, WorkspaceError};

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

#[pyclass(frozen, skip_from_py_object)]
#[derive(Clone, Debug)]
struct NativeWorkspaceMutation {
    context: MutationContext,
}

#[pymethods]
impl NativeWorkspaceMutation {
    #[getter]
    fn mode(&self) -> &str {
        &self.context.mode
    }

    #[getter]
    fn mode_generation(&self) -> u64 {
        self.context.mode_generation
    }

    #[getter]
    fn policy_generation(&self) -> u64 {
        self.context.policy_generation
    }
}

#[pyclass(frozen, skip_from_py_object)]
#[derive(Clone, Debug)]
struct NativeWorkspaceCancellation {
    inner: Cancellation,
}

#[pymethods]
impl NativeWorkspaceCancellation {
    #[new]
    fn new() -> Self {
        Self {
            inner: Cancellation::new(),
        }
    }

    fn cancel(&self) {
        self.inner.cancel();
    }
}

impl NativeWorkspaceCancellation {
    fn token(&self) -> Cancellation {
        self.inner.clone()
    }
}

impl NativeWorkspace {
    fn run<T, U, F>(&self, py: Python<'_>, operation: F) -> PyResult<U>
    where
        F: FnOnce(Workspace) -> Result<T, WorkspaceError> + Send,
        U: From<T> + Send,
    {
        let workspace = self.inner.clone();
        py.detach(move || operation(workspace).map(U::from).map_err(to_python_error))
    }

    fn write<F>(
        &self,
        py: Python<'_>,
        cancellation: &NativeWorkspaceCancellation,
        operation: F,
    ) -> PyResult<NativeEditResult>
    where
        F: FnOnce(Workspace, MutationContext) -> Result<EditResult, WorkspaceError> + Send,
    {
        let cancellation = cancellation.token();
        self.run(py, move |workspace| {
            let context = MutationContext::write(workspace.policy()?, cancellation);
            operation(workspace, context)
        })
    }

    fn mutate<F>(
        &self,
        py: Python<'_>,
        mutation: &NativeWorkspaceMutation,
        cancellation: &NativeWorkspaceCancellation,
        operation: F,
    ) -> PyResult<NativeEditResult>
    where
        F: FnOnce(Workspace, MutationContext) -> Result<EditResult, WorkspaceError> + Send,
    {
        let mut context = mutation.context.clone();
        context.cancellation = cancellation.token();
        self.run(py, move |workspace| operation(workspace, context))
    }
}

pub(crate) fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    register_exports!(module, classes [NativeWorkspace, NativeWorkspaceMutation, NativeWorkspaceCancellation], functions []);
    errors::register(module)?;
    lifecycle::register(module)?;
    reads::register(module)?;
    writes::register(module)?;
    edits::register(module)
}
