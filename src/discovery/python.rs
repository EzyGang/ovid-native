use std::path::PathBuf;
use std::time::Duration;

use pyo3::create_exception;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;

use crate::discovery::{
    DiscoveryError, NamedFileRequest, NamedFileResult, discover_named_files, find_ancestor_entry,
    read_text_files,
};
use crate::workspace::{Cancellation, WorkCompletion, WorkControl};

create_exception!(_native, NativeDiscoveryError, PyException);
create_exception!(
    _native,
    NativeDiscoveryConfigurationError,
    NativeDiscoveryError
);
create_exception!(_native, NativeDiscoveryPathError, NativeDiscoveryError);
create_exception!(_native, NativeDiscoveryReadError, NativeDiscoveryError);
create_exception!(
    _native,
    NativeDiscoveryEncodingError,
    NativeDiscoveryReadError
);
create_exception!(_native, NativeDiscoveryCancelledError, NativeDiscoveryError);

#[pyclass]
struct NativeDiscoveryCancellation {
    inner: Cancellation,
}

#[pymethods]
impl NativeDiscoveryCancellation {
    #[new]
    fn new() -> Self {
        Self {
            inner: Cancellation::new(),
        }
    }

    fn cancel(&self) {
        self.inner.cancel();
    }

    #[must_use]
    fn is_cancelled(&self) -> bool {
        self.inner.is_cancelled()
    }
}

#[pyfunction]
fn discovery_find_ancestor_entry(
    py: Python<'_>,
    start: String,
    name: String,
) -> PyResult<Option<String>> {
    py.detach(move || find_ancestor_entry(&PathBuf::from(start), &name))
        .map(|path| path.map(|value| value.to_string_lossy().into_owned()))
        .map_err(to_python_error)
}

#[pyfunction]
fn discovery_read_text_files(
    py: Python<'_>,
    paths: Vec<String>,
    cancellation: PyRef<'_, NativeDiscoveryCancellation>,
) -> PyResult<Vec<(String, String)>> {
    let control = WorkControl::new(cancellation.inner.clone(), None);
    py.detach(move || read_text_files(paths, &control))
        .map_err(to_python_error)
}

#[pyfunction]
fn discovery_find_named_files(
    py: Python<'_>,
    root: String,
    filename: String,
    max_depth: usize,
    limit: usize,
    timeout_seconds: f64,
    cancellation: PyRef<'_, NativeDiscoveryCancellation>,
) -> PyResult<(Vec<String>, String)> {
    if !timeout_seconds.is_finite() || timeout_seconds <= 0.0 || timeout_seconds > 30.0 {
        return Err(NativeDiscoveryConfigurationError::new_err(
            "named file discovery timeout must be greater than zero and at most 30 seconds",
        ));
    }
    let request = NamedFileRequest {
        filename,
        max_depth,
        limit,
    };
    let control = WorkControl::new(
        cancellation.inner.clone(),
        Some(Duration::from_secs_f64(timeout_seconds)),
    );

    py.detach(move || discover_named_files(&PathBuf::from(root), &request, &control))
        .map(result_to_native)
        .map_err(to_python_error)
}

pub(crate) fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<NativeDiscoveryCancellation>()?;
    module.add(
        "NativeDiscoveryError",
        module.py().get_type::<NativeDiscoveryError>(),
    )?;
    module.add(
        "NativeDiscoveryConfigurationError",
        module.py().get_type::<NativeDiscoveryConfigurationError>(),
    )?;
    module.add(
        "NativeDiscoveryPathError",
        module.py().get_type::<NativeDiscoveryPathError>(),
    )?;
    module.add(
        "NativeDiscoveryReadError",
        module.py().get_type::<NativeDiscoveryReadError>(),
    )?;
    module.add(
        "NativeDiscoveryEncodingError",
        module.py().get_type::<NativeDiscoveryEncodingError>(),
    )?;
    module.add(
        "NativeDiscoveryCancelledError",
        module.py().get_type::<NativeDiscoveryCancelledError>(),
    )?;
    module.add_function(wrap_pyfunction!(discovery_find_ancestor_entry, module)?)?;
    module.add_function(wrap_pyfunction!(discovery_read_text_files, module)?)?;
    module.add_function(wrap_pyfunction!(discovery_find_named_files, module)?)?;
    Ok(())
}

fn result_to_native(result: NamedFileResult) -> (Vec<String>, String) {
    let completion = match result.completion {
        WorkCompletion::Complete => "complete",
        WorkCompletion::FileLimitReached => "file_limit_reached",
        WorkCompletion::DeadlineReached => "deadline_reached",
    };
    (result.paths, completion.to_owned())
}

fn to_python_error(error: DiscoveryError) -> PyErr {
    match error {
        DiscoveryError::Configuration(message) => {
            NativeDiscoveryConfigurationError::new_err(message)
        }
        DiscoveryError::Path(message) => NativeDiscoveryPathError::new_err(message),
        DiscoveryError::Read(message) => NativeDiscoveryReadError::new_err(message),
        DiscoveryError::Encoding(message) => NativeDiscoveryEncodingError::new_err(message),
        DiscoveryError::Cancelled => {
            NativeDiscoveryCancelledError::new_err("file discovery was cancelled")
        }
    }
}
