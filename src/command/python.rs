use std::time::Duration;

use pyo3::prelude::*;

use crate::command::jobs::{NativeCommandJobRequest, NativeCommandJobs};
use crate::command::records::reap_orphans;
use crate::command::{CommandCancellation, CommandError, CommandRequest, CommandResult, execute};
use crate::python_boundary::{python_errors, register_exports};
use crate::workspace::NativeWorkspace;

type NativeReapedProcess = (u32, Option<String>, String);

python_errors! {
    CommandError;
    NativeCommandConfigurationError: error @ CommandError::Configuration(_) => error.to_string(),
    NativeCommandPathError: error @ CommandError::Path(_) => error.to_string(),
    NativeCommandExecutionError: error @ (CommandError::Execution(_) | CommandError::Output(_)) => error.to_string(),
    NativeCommandCancelledError: error @ CommandError::Cancelled => error.to_string(),
    NativeCommandClosedError: error @ CommandError::Closed => error.to_string(),
    NativeCommandJobLimitError: error @ CommandError::JobLimit(_) => error.to_string(),
    NativeCommandJobNotFoundError: error @ CommandError::JobNotFound(_) => error.to_string(),
}

#[pyclass(frozen, skip_from_py_object)]
#[derive(Clone)]
pub(super) struct NativeCommandCancellation {
    pub(super) inner: CommandCancellation,
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
    #[allow(clippy::too_many_arguments)]
    fn new(
        command: String,
        cwd: Option<String>,
        env: Vec<(String, String)>,
        timeout_seconds: f64,
        max_output_bytes: usize,
        max_head_bytes: usize,
        cancellation: PyRef<'_, NativeCommandCancellation>,
        owner_tag: Option<String>,
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
                max_head_bytes,
                cancellation: cancellation.inner.clone(),
                owner_tag,
            },
        })
    }
}

#[pyfunction]
fn command_execute(
    py: Python<'_>,
    workspace: PyRef<'_, NativeWorkspace>,
    request: PyRef<'_, NativeCommandRequest>,
) -> PyResult<(String, String, u64, Option<i32>, bool, u64)> {
    let workspace = workspace.inner.clone();
    let request = request.inner.clone();
    py.detach(move || execute(&workspace, request))
        .map(result_tuple)
        .map_err(to_python_error)
}

#[pyfunction]
fn command_reap_orphans(
    py: Python<'_>,
    workspace: PyRef<'_, NativeWorkspace>,
) -> PyResult<(Vec<NativeReapedProcess>, u64)> {
    let workspace = workspace.inner.clone();
    py.detach(move || reap_orphans(&workspace))
        .map(|report| {
            let stopped = report.stopped.into_iter();
            let stopped = stopped
                .map(|process| (process.pid, process.owner_tag, process.command_hash_prefix));
            (stopped.collect(), report.skipped_live_owners)
        })
        .map_err(to_python_error)
}

pub(crate) fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    register_exports!(module,
        classes [
            NativeCommandCancellation,
            NativeCommandRequest,
            NativeCommandJobRequest,
            NativeCommandJobs,
        ],
        functions [command_execute, command_reap_orphans]
    );
    add_exceptions(module)
}

fn result_tuple(result: CommandResult) -> (String, String, u64, Option<i32>, bool, u64) {
    (
        result.head,
        result.tail,
        result.omitted_bytes,
        result.exit_code,
        result.timed_out,
        result.wall_time_ms,
    )
}
