use std::time::Duration;

use pyo3::prelude::*;

use crate::command::{CommandCancellation, CommandError, CommandRequest, CommandResult, execute};
use crate::python_boundary::{python_errors, register_exports};
use crate::workspace::NativeWorkspace;

python_errors! {
    CommandError;
    NativeCommandConfigurationError: CommandError::Configuration(message) => message,
    NativeCommandPathError: CommandError::Path(message) => message,
    NativeCommandExecutionError: CommandError::Execution(message) | CommandError::Output(message) => message,
    NativeCommandCancelledError: CommandError::Cancelled => "command execution cancelled",
    NativeCommandClosedError: CommandError::Closed => "workspace session is closed",
}

#[pyclass(frozen, skip_from_py_object)]
#[derive(Clone)]
struct NativeCommandCancellation {
    inner: CommandCancellation,
}

#[pymethods]
impl NativeCommandCancellation {
    #[new]
    fn new() -> Self {
        Self {
            inner: CommandCancellation::new(),
        }
    }

    fn cancel(&self) {
        self.inner.cancel();
    }
}

#[pyclass(frozen)]
struct NativeCommandRequest {
    inner: CommandRequest,
}

#[pymethods]
impl NativeCommandRequest {
    #[new]
    fn new(
        command: String,
        cwd: Option<String>,
        env: Vec<(String, String)>,
        timeout_seconds: f64,
        max_output_bytes: usize,
        cancellation: PyRef<'_, NativeCommandCancellation>,
    ) -> PyResult<Self> {
        if !timeout_seconds.is_finite() || !(1.0..=3_600.0).contains(&timeout_seconds) {
            return Err(NativeCommandConfigurationError::new_err(
                "command timeout must be between 1 and 3600 seconds",
            ));
        }
        Ok(Self {
            inner: CommandRequest {
                command,
                cwd,
                env,
                timeout: Duration::from_secs_f64(timeout_seconds),
                max_output_bytes,
                cancellation: cancellation.inner.clone(),
            },
        })
    }
}

#[pyfunction]
fn command_execute(
    py: Python<'_>,
    workspace: PyRef<'_, NativeWorkspace>,
    request: PyRef<'_, NativeCommandRequest>,
) -> PyResult<(String, Option<i32>, bool, bool, u64)> {
    let workspace = workspace.inner.clone();
    let request = request.inner.clone();
    py.detach(move || execute(&workspace, request))
        .map(result_tuple)
        .map_err(to_python_error)
}

pub(crate) fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    register_exports!(module,
        classes [NativeCommandCancellation, NativeCommandRequest],
        functions [command_execute]
    );
    add_exceptions(module)
}

fn result_tuple(result: CommandResult) -> (String, Option<i32>, bool, bool, u64) {
    (
        result.output,
        result.exit_code,
        result.timed_out,
        result.truncated,
        result.wall_time_ms,
    )
}
