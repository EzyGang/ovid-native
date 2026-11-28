use std::time::Duration;

use ovid_command_process::TerminalSize;
use pyo3::prelude::*;

use crate::command::CommandError;
use crate::command::jobs::{
    CommandJobLimits, CommandJobRegistry, CommandJobRequest, CommandJobSignal, CommandJobSnapshot,
};
use crate::command::python::{NativeCommandCancellation, to_python_error};
use crate::workspace::NativeWorkspace;

type SnapshotTuple = (
    u64,
    &'static str,
    Option<i32>,
    String,
    String,
    u64,
    u64,
    u64,
    u64,
);

#[pyclass(frozen)]
pub(in crate::command) struct NativeCommandJobRequest {
    inner: CommandJobRequest,
}

#[pymethods]
impl NativeCommandJobRequest {
    #[new]
    fn new(
        command: String,
        cwd: Option<String>,
        env: Vec<(String, String)>,
        timeout_seconds: Option<f64>,
        terminal: Option<(u16, u16)>,
        max_buffer_bytes: usize,
        owner_tag: Option<String>,
    ) -> PyResult<Self> {
        let timeout = timeout_seconds
            .map(Duration::try_from_secs_f64)
            .transpose()
            .map_err(|error| to_python_error(CommandError::Configuration(error.to_string())))?;
        Ok(Self {
            inner: CommandJobRequest {
                command,
                cwd,
                env,
                timeout,
                terminal: terminal.map(|(cols, rows)| TerminalSize { cols, rows }),
                max_buffer_bytes,
                owner_tag,
            },
        })
    }
}

#[pyclass(frozen)]
pub(in crate::command) struct NativeCommandJobs {
    registry: CommandJobRegistry,
}

#[pymethods]
impl NativeCommandJobs {
    #[new]
    fn new(workspace: PyRef<'_, NativeWorkspace>) -> Self {
        Self {
            registry: CommandJobRegistry::new(workspace.inner.clone(), CommandJobLimits::default()),
        }
    }

    fn start(
        &self,
        py: Python<'_>,
        request: PyRef<'_, NativeCommandJobRequest>,
    ) -> PyResult<SnapshotTuple> {
        let request = request.inner.clone();
        py.detach(|| self.registry.start(request))
            .map(snapshot_tuple)
            .map_err(to_python_error)
    }

    fn poll(
        &self,
        py: Python<'_>,
        job_id: u64,
        offset: u64,
        strip_ansi: bool,
    ) -> PyResult<SnapshotTuple> {
        py.detach(|| self.registry.poll(job_id, offset, strip_ansi))
            .map(snapshot_tuple)
            .map_err(to_python_error)
    }

    fn write(&self, py: Python<'_>, job_id: u64, data: String) -> PyResult<()> {
        py.detach(|| self.registry.write(job_id, data.as_bytes()))
            .map_err(to_python_error)
    }

    fn resize(&self, py: Python<'_>, job_id: u64, cols: u16, rows: u16) -> PyResult<()> {
        py.detach(|| self.registry.resize(job_id, TerminalSize { cols, rows }))
            .map_err(to_python_error)
    }

    fn signal(&self, py: Python<'_>, job_id: u64, signal: &str) -> PyResult<()> {
        let signal = CommandJobSignal::parse(signal).map_err(to_python_error)?;
        py.detach(|| self.registry.signal(job_id, signal))
            .map_err(to_python_error)
    }

    fn wait(
        &self,
        py: Python<'_>,
        job_id: u64,
        offset: u64,
        strip_ansi: bool,
        timeout_seconds: f64,
        cancellation: PyRef<'_, NativeCommandCancellation>,
    ) -> PyResult<SnapshotTuple> {
        let timeout = Duration::try_from_secs_f64(timeout_seconds)
            .map_err(|error| to_python_error(CommandError::Configuration(error.to_string())))?;
        let cancellation = cancellation.inner.clone();
        py.detach(|| {
            self.registry
                .wait(job_id, offset, strip_ansi, timeout, &cancellation)
        })
        .map(snapshot_tuple)
        .map_err(to_python_error)
    }

    fn list_jobs(&self, py: Python<'_>) -> PyResult<Vec<SnapshotTuple>> {
        py.detach(|| self.registry.list())
            .map(|snapshots| snapshots.into_iter().map(snapshot_tuple).collect())
            .map_err(to_python_error)
    }

    fn close(&self, py: Python<'_>) {
        py.detach(|| self.registry.close());
    }
}

fn snapshot_tuple(snapshot: CommandJobSnapshot) -> SnapshotTuple {
    (
        snapshot.job_id,
        snapshot.status.as_str(),
        snapshot.exit_code,
        snapshot.head,
        snapshot.tail,
        snapshot.omitted_bytes,
        snapshot.next_offset,
        snapshot.truncated_before,
        snapshot.wall_time_ms,
    )
}
