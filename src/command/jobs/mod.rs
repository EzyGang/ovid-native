mod job;
mod launch;
mod python;
mod ring;
#[cfg(test)]
mod tests;
mod types;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use ovid_command_process::TerminalSize;

use crate::command::jobs::job::CommandJob;
use crate::command::jobs::launch::launch;
use crate::command::jobs::ring::{BufferBudget, OutputRing};
use crate::command::jobs::types::JOB_HEAD_BYTES;
use crate::command::{CommandCancellation, CommandError, resolve_cwd};
use crate::workspace::Workspace;

pub(super) use python::{NativeCommandJobRequest, NativeCommandJobs};
pub(crate) use types::{
    CommandJobLimits, CommandJobRequest, CommandJobSignal, CommandJobSnapshot, CommandJobStatus,
};

/// Background command jobs of one workspace session.
///
/// Each job owns a process scope. A job keeps its scope after its command ends, so processes
/// that the command starts in the background keep running. A kill or terminate signal, the end
/// of retention, [`CommandJobRegistry::close`], and `Drop` stop every process of a job.
pub(crate) struct CommandJobRegistry {
    workspace: Workspace,
    limits: CommandJobLimits,
    budget: Arc<BufferBudget>,
    state: Mutex<RegistryState>,
}

struct RegistryState {
    next_id: u64,
    jobs: BTreeMap<u64, Arc<CommandJob>>,
    closed: bool,
}

impl CommandJobRegistry {
    pub(crate) fn new(workspace: Workspace, limits: CommandJobLimits) -> Self {
        Self {
            workspace,
            budget: Arc::new(BufferBudget::new(limits.buffer_limit)),
            limits,
            state: Mutex::new(RegistryState {
                next_id: 0,
                jobs: BTreeMap::new(),
                closed: false,
            }),
        }
    }

    pub(crate) fn start(
        &self,
        request: CommandJobRequest,
    ) -> Result<CommandJobSnapshot, CommandError> {
        request.validate(&self.limits)?;
        self.workspace
            .ensure_open()
            .map_err(CommandError::from_workspace)?;
        let cwd = resolve_cwd(self.workspace.root(), request.cwd.as_deref())?;
        let mut state = self.open_state()?;
        let running = state.jobs.values().filter(|job| job.is_running()).count();
        if running >= self.limits.max_running {
            return Err(CommandError::JobLimit(format!(
                "at most {} command jobs can run at the same time",
                self.limits.max_running
            )));
        }

        let id = state.next_id + 1;
        let output = OutputRing::new(
            JOB_HEAD_BYTES,
            request.max_buffer_bytes,
            Arc::clone(&self.budget),
        );
        let job = launch(id, request, cwd, output)?;
        state.next_id = id;
        state.jobs.insert(id, Arc::clone(&job));
        Ok(job.snapshot(0, false))
    }

    pub(crate) fn poll(
        &self,
        id: u64,
        offset: u64,
        strip_ansi: bool,
    ) -> Result<CommandJobSnapshot, CommandError> {
        Ok(self.job(id)?.snapshot(offset, strip_ansi))
    }

    pub(crate) fn write(&self, id: u64, bytes: &[u8]) -> Result<(), CommandError> {
        self.job(id)?.write(bytes)
    }

    pub(crate) fn resize(&self, id: u64, size: TerminalSize) -> Result<(), CommandError> {
        self.job(id)?.resize(size)
    }

    pub(crate) fn signal(&self, id: u64, signal: CommandJobSignal) -> Result<(), CommandError> {
        self.job(id)?.stop(signal)
    }

    pub(crate) fn wait(
        &self,
        id: u64,
        offset: u64,
        strip_ansi: bool,
        timeout: Duration,
        cancellation: &CommandCancellation,
    ) -> Result<CommandJobSnapshot, CommandError> {
        let job = self.job(id)?;
        job.wait(timeout, cancellation)?;
        Ok(job.snapshot(offset, strip_ansi))
    }

    /// Returns the state of every job without output.
    pub(crate) fn list(&self) -> Result<Vec<CommandJobSnapshot>, CommandError> {
        let state = self.open_state()?;
        Ok(state
            .jobs
            .values()
            .map(|job| job.snapshot(u64::MAX, false))
            .collect())
    }

    /// Stops every job and refuses later operations.
    pub(crate) fn close(&self) {
        let jobs = {
            let mut state = self.state();
            state.closed = true;
            std::mem::take(&mut state.jobs)
        };
        for job in jobs.values() {
            discard(job);
        }
    }

    fn job(&self, id: u64) -> Result<Arc<CommandJob>, CommandError> {
        self.open_state()?
            .jobs
            .get(&id)
            .cloned()
            .ok_or(CommandError::JobNotFound(id))
    }

    fn open_state(&self) -> Result<MutexGuard<'_, RegistryState>, CommandError> {
        let mut state = self.state();
        if state.closed {
            return Err(CommandError::Closed);
        }
        let now = Instant::now();
        state.jobs.retain(|_, job| {
            let expired = job.expired(self.limits.retention, now);
            if expired {
                discard(job);
            }
            !expired
        });
        Ok(state)
    }

    fn state(&self) -> MutexGuard<'_, RegistryState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Drop for CommandJobRegistry {
    fn drop(&mut self) {
        self.close();
    }
}

fn discard(job: &CommandJob) {
    // A job that leaves the registry cannot report a failure to stop, and its scope still stops
    // every remaining process when it drops.
    let _ = job.stop(CommandJobSignal::Kill);
}
