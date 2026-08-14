mod content;
mod control;
mod path;
mod scan;
#[cfg(test)]
mod tests;
mod types;
mod write;

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use pyo3::create_exception;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;

pub(crate) use content::{ReadExtent, read_content};
pub(crate) use control::{Cancellation, WorkControl, WorkStopped};
pub(crate) use types::{
    MetadataLevel, ScanFileKind, ScanOrder, ScanRequest, ScanResult, WorkCompletion,
    WorkspaceEntry, WorkspaceFileType,
};
pub(crate) use write::{preflight_write, replace_file, sha256};

create_exception!(_native, NativeWorkspaceConfigurationError, PyException);
create_exception!(_native, NativeWorkspacePathError, PyException);
create_exception!(_native, NativeWorkspaceClosedError, PyException);

#[derive(Debug)]
pub(crate) enum WorkspaceError {
    Configuration(String),
    Path(String),
    Read(String),
    Stale(String),
    Write(String),
    Cancelled,
    Deadline,
}

#[derive(Debug)]
pub(crate) struct Workspace {
    canonical_root: std::path::PathBuf,
    id: String,
    revision: AtomicU64,
    closed: AtomicBool,
    cancellation: Cancellation,
    active_operations: Mutex<usize>,
    operations_finished: Condvar,
}

impl Workspace {
    #[cfg(test)]
    pub(crate) fn new(value: &str) -> Result<Self, WorkspaceError> {
        Self::with_id(value, "standalone")
    }

    pub(crate) fn with_id(value: &str, id: &str) -> Result<Self, WorkspaceError> {
        if id.is_empty() {
            return Err(WorkspaceError::Configuration(
                "workspace session identity must not be empty".to_owned(),
            ));
        }

        Ok(Self {
            canonical_root: path::canonical_root(value)?,
            id: id.to_owned(),
            revision: AtomicU64::new(0),
            closed: AtomicBool::new(false),
            cancellation: Cancellation::new(),
            active_operations: Mutex::new(0),
            operations_finished: Condvar::new(),
        })
    }

    #[cfg(test)]
    pub(crate) fn from_canonical(root: &Path) -> Self {
        Self {
            canonical_root: root.to_path_buf(),
            id: "standalone".to_owned(),
            revision: AtomicU64::new(0),
            closed: AtomicBool::new(false),
            cancellation: Cancellation::new(),
            active_operations: Mutex::new(0),
            operations_finished: Condvar::new(),
        }
    }

    pub(crate) fn root(&self) -> &Path {
        &self.canonical_root
    }

    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    pub(crate) fn revision(&self) -> u64 {
        self.revision.load(Ordering::Acquire)
    }

    pub(crate) fn mark_changed(&self) {
        self.revision.fetch_add(1, Ordering::AcqRel);
    }

    pub(crate) fn ensure_open(&self) -> bool {
        !self.closed.load(Ordering::Acquire)
    }

    pub(crate) fn begin(self: &Arc<Self>) -> Option<WorkspaceOperationGuard> {
        let mut active = self.active_operations.lock().ok()?;
        if !self.ensure_open() {
            return None;
        }

        *active += 1;
        Some(WorkspaceOperationGuard {
            workspace: self.clone(),
        })
    }

    pub(crate) fn cancellation(&self) -> &Cancellation {
        &self.cancellation
    }

    pub(crate) fn close(&self) {
        self.closed.store(true, Ordering::Release);
        self.cancellation.cancel();
        let mut active = match self.active_operations.lock() {
            Ok(active) => active,
            Err(error) => error.into_inner(),
        };
        while *active > 0 {
            active = match self.operations_finished.wait(active) {
                Ok(active) => active,
                Err(error) => error.into_inner(),
            };
        }
    }

    pub(crate) fn scan(
        &self,
        request: &ScanRequest,
        control: &WorkControl,
    ) -> Result<ScanResult, WorkspaceError> {
        scan::scan(&self.canonical_root, request, control)
    }
}

#[derive(Debug)]
pub(crate) struct WorkspaceOperationGuard {
    workspace: Arc<Workspace>,
}

impl Drop for WorkspaceOperationGuard {
    fn drop(&mut self) {
        let mut active = match self.workspace.active_operations.lock() {
            Ok(active) => active,
            Err(error) => error.into_inner(),
        };
        *active = active.saturating_sub(1);
        if *active == 0 {
            self.workspace.operations_finished.notify_all();
        }
    }
}

#[pyclass(frozen, skip_from_py_object)]
#[derive(Clone, Debug)]
pub(crate) struct NativeWorkspace {
    pub(crate) inner: Arc<Workspace>,
}

impl NativeWorkspace {
    fn new(workspace: Workspace) -> Self {
        Self {
            inner: Arc::new(workspace),
        }
    }
}

#[pymethods]
impl NativeWorkspace {
    #[getter]
    fn root(&self) -> String {
        self.inner.root().to_string_lossy().into_owned()
    }

    #[getter]
    fn session_id(&self) -> &str {
        self.inner.id()
    }

    #[getter]
    fn revision(&self) -> u64 {
        self.inner.revision()
    }
}

#[pyfunction]
fn workspace_create(root: String, session_id: String) -> PyResult<NativeWorkspace> {
    Workspace::with_id(&root, &session_id)
        .map(NativeWorkspace::new)
        .map_err(workspace_python_error)
}

#[pyfunction]
fn workspace_close(py: Python<'_>, workspace: PyRef<'_, NativeWorkspace>) {
    let workspace = workspace.inner.clone();
    py.detach(move || workspace.close());
}

pub(crate) fn register_module(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<NativeWorkspace>()?;
    module.add_function(wrap_pyfunction!(workspace_create, module)?)?;
    module.add_function(wrap_pyfunction!(workspace_close, module)?)?;
    module.add(
        "NativeWorkspaceConfigurationError",
        module.py().get_type::<NativeWorkspaceConfigurationError>(),
    )?;
    module.add(
        "NativeWorkspacePathError",
        module.py().get_type::<NativeWorkspacePathError>(),
    )?;
    module.add(
        "NativeWorkspaceClosedError",
        module.py().get_type::<NativeWorkspaceClosedError>(),
    )?;

    Ok(())
}

pub(crate) fn closed_python_error() -> PyErr {
    NativeWorkspaceClosedError::new_err("workspace session is closed")
}

fn workspace_python_error(error: WorkspaceError) -> PyErr {
    match error {
        WorkspaceError::Path(message) => NativeWorkspacePathError::new_err(message),
        WorkspaceError::Configuration(message)
        | WorkspaceError::Read(message)
        | WorkspaceError::Stale(message)
        | WorkspaceError::Write(message) => NativeWorkspaceConfigurationError::new_err(message),
        WorkspaceError::Cancelled => {
            NativeWorkspaceConfigurationError::new_err("workspace operation cancelled")
        }
        WorkspaceError::Deadline => {
            NativeWorkspaceConfigurationError::new_err("workspace operation reached its deadline")
        }
    }
}
