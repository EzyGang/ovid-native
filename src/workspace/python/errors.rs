use pyo3::{
    create_exception,
    exceptions::{PyException, PyValueError},
    prelude::*,
};

use crate::python_boundary::add_exceptions;
use crate::workspace::WorkspaceError;
use crate::workspace::python::results::{NativeFileChange, native_items};

create_exception!(_native, NativeWorkspaceReadError, PyException);
create_exception!(
    _native,
    NativeWorkspaceEncodingError,
    NativeWorkspaceReadError
);
create_exception!(
    _native,
    NativeWorkspaceBinaryFileError,
    NativeWorkspaceReadError
);
create_exception!(_native, NativeWorkspaceLimitError, PyException);
create_exception!(
    _native,
    NativeWorkspaceObservationNotFoundError,
    PyException
);
create_exception!(
    _native,
    NativeWorkspaceObservationCollisionError,
    PyException
);
create_exception!(_native, NativeWorkspaceUnseenLineError, PyException);
create_exception!(
    _native,
    NativeWorkspaceObservedLineChangedError,
    PyException
);
create_exception!(_native, NativeWorkspaceStaleError, PyException);
create_exception!(_native, NativeWorkspaceEditModeError, PyException);
create_exception!(_native, NativeWorkspacePatchError, PyException);
create_exception!(_native, NativeWorkspacePartialCommitError, PyException);
create_exception!(_native, NativeWorkspaceWriteError, PyException);
create_exception!(_native, NativeWorkspacePathError, PyException);
create_exception!(_native, NativeWorkspaceClosedError, PyException);

pub(super) fn to_python_error(error: WorkspaceError) -> PyErr {
    match error {
        WorkspaceError::Configuration(message) => PyValueError::new_err(message),
        WorkspaceError::Path(message) => NativeWorkspacePathError::new_err(message),
        WorkspaceError::Read(message) => NativeWorkspaceReadError::new_err(message),
        WorkspaceError::Encoding(message) => NativeWorkspaceEncodingError::new_err(message),
        WorkspaceError::Binary(message) => NativeWorkspaceBinaryFileError::new_err(message),
        WorkspaceError::Limit(message) => NativeWorkspaceLimitError::new_err(message),
        WorkspaceError::ObservationNotFound(message) => {
            NativeWorkspaceObservationNotFoundError::new_err(message)
        }
        WorkspaceError::ObservationCollision(message) => {
            NativeWorkspaceObservationCollisionError::new_err(message)
        }
        WorkspaceError::UnseenLine(message) => NativeWorkspaceUnseenLineError::new_err(message),
        WorkspaceError::ObservedLineChanged(message) => {
            NativeWorkspaceObservedLineChangedError::new_err(message)
        }
        WorkspaceError::Stale(message) => NativeWorkspaceStaleError::new_err(message),
        WorkspaceError::EditMode(message) => NativeWorkspaceEditModeError::new_err(message),
        WorkspaceError::Patch(message) => NativeWorkspacePatchError::new_err(message),
        WorkspaceError::PartialCommit {
            landed,
            pending,
            changes,
        } => NativeWorkspacePartialCommitError::new_err((
            "workspace patch committed only part of its operations",
            landed,
            pending,
            native_items::<_, NativeFileChange>(changes),
        )),
        WorkspaceError::Write(message) => NativeWorkspaceWriteError::new_err(message),
        WorkspaceError::Cancelled => {
            NativeWorkspaceWriteError::new_err("workspace operation was cancelled")
        }
        WorkspaceError::Closed => NativeWorkspaceClosedError::new_err("workspace is closed"),
        WorkspaceError::Deadline => {
            NativeWorkspaceReadError::new_err("workspace operation reached its deadline")
        }
    }
}

pub(super) fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    add_exceptions(
        module,
        [
            module.py().get_type::<NativeWorkspaceReadError>(),
            module.py().get_type::<NativeWorkspaceEncodingError>(),
            module.py().get_type::<NativeWorkspaceBinaryFileError>(),
            module.py().get_type::<NativeWorkspaceLimitError>(),
            module
                .py()
                .get_type::<NativeWorkspaceObservationNotFoundError>(),
            module
                .py()
                .get_type::<NativeWorkspaceObservationCollisionError>(),
            module.py().get_type::<NativeWorkspaceUnseenLineError>(),
            module
                .py()
                .get_type::<NativeWorkspaceObservedLineChangedError>(),
            module.py().get_type::<NativeWorkspaceStaleError>(),
            module.py().get_type::<NativeWorkspaceEditModeError>(),
            module.py().get_type::<NativeWorkspacePatchError>(),
            module.py().get_type::<NativeWorkspacePartialCommitError>(),
            module.py().get_type::<NativeWorkspaceWriteError>(),
            module.py().get_type::<NativeWorkspacePathError>(),
            module.py().get_type::<NativeWorkspaceClosedError>(),
        ],
    )
}
