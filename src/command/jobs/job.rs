use std::io::{ErrorKind, Read, Write};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use ovid_command_process::{ProcessScope, Terminal, TerminalSize};

use crate::command::jobs::launch::JobEnd;
use crate::command::jobs::ring::{OutputRing, render};
use crate::command::jobs::{CommandJobSignal, CommandJobSnapshot, CommandJobStatus};
use crate::command::{CommandCancellation, CommandError};

/// One background command and every process that it starts.
pub(super) struct CommandJob {
    pub id: u64,
    pub scope: Arc<ProcessScope>,
    pub cancellation: CommandCancellation,
    pub deadline: Option<Instant>,
    state: Mutex<JobState>,
    changed: Condvar,
    stdin: Mutex<Option<Box<dyn Write + Send>>>,
    terminal: Mutex<Option<Terminal>>,
    started: Instant,
}

struct JobState {
    status: CommandJobStatus,
    exit_code: Option<i32>,
    output: OutputRing,
    output_closed: bool,
    finished: Option<Instant>,
}

impl CommandJob {
    pub(super) fn new(id: u64, output: OutputRing, deadline: Option<Instant>) -> Self {
        Self {
            id,
            scope: Arc::new(ProcessScope::default()),
            cancellation: CommandCancellation::new(),
            deadline,
            state: Mutex::new(JobState {
                status: CommandJobStatus::Running,
                exit_code: None,
                output,
                output_closed: false,
                finished: None,
            }),
            changed: Condvar::new(),
            stdin: Mutex::new(None),
            terminal: Mutex::new(None),
            started: Instant::now(),
        }
    }

    pub(super) fn attach(&self, stdin: Box<dyn Write + Send>, terminal: Option<Terminal>) {
        *self.stdin() = Some(stdin);
        *self.terminal() = terminal;
    }

    pub(super) fn read_output<R>(&self, mut reader: R)
    where
        R: Read,
    {
        let mut chunk = vec![0u8; 65_536];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) => break,
                Ok(read) => self.state().output.push(&chunk[..read]),
                Err(error) if error.kind() == ErrorKind::Interrupted => (),
                Err(error) => {
                    self.report(format_args!("cannot read job output: {error}"));
                    break;
                }
            }
        }
        self.state().output_closed = true;
        self.changed.notify_all();
    }

    pub(super) fn finish(&self, end: JobEnd) {
        match end {
            JobEnd::Exited(code) => self.mark(CommandJobStatus::Exited, Some(code)),
            JobEnd::Failed(message) => {
                self.report(message);
                self.mark(CommandJobStatus::Failed, None);
            }
            JobEnd::TimedOut => {
                self.mark(CommandJobStatus::TimedOut, None);
                self.cancellation.cancel();
                if let Err(error) = self.scope.terminate(Duration::ZERO) {
                    self.report(format_args!("cannot stop job processes: {error}"));
                }
            }
            JobEnd::Stopped => (),
        }
        // A closed pseudoconsole ends the output stream on Windows.
        drop(self.terminal().take());
    }

    pub(super) fn stop(&self, signal: CommandJobSignal) -> Result<(), CommandError> {
        let grace = match signal {
            CommandJobSignal::Interrupt => {
                self.require_running()?;
                if self.terminal().is_none() {
                    return Err(CommandError::Configuration(
                        "only a terminal job accepts an interrupt".to_owned(),
                    ));
                }
                return self.write(b"\x03");
            }
            CommandJobSignal::Terminate => Duration::from_millis(500),
            CommandJobSignal::Kill => Duration::ZERO,
        };
        self.mark(CommandJobStatus::Killed, None);
        self.cancellation.cancel();
        drop(self.stdin().take());
        self.scope
            .terminate(grace)
            .map_err(|error| CommandError::Execution(error.to_string()))
    }

    pub(super) fn write(&self, bytes: &[u8]) -> Result<(), CommandError> {
        self.require_running()?;
        let mut stdin = self.stdin();
        let Some(stdin) = stdin.as_mut() else {
            return Err(CommandError::Execution(
                "command job input is closed".to_owned(),
            ));
        };
        stdin
            .write_all(bytes)
            .and_then(|()| stdin.flush())
            .map_err(|error| CommandError::Execution(error.to_string()))
    }

    pub(super) fn resize(&self, size: TerminalSize) -> Result<(), CommandError> {
        self.require_running()?;
        let terminal = self.terminal();
        let Some(terminal) = terminal.as_ref() else {
            return Err(CommandError::Configuration(
                "command job has no terminal".to_owned(),
            ));
        };
        terminal
            .resize(size)
            .map_err(|error| CommandError::Execution(error.to_string()))
    }

    /// Blocks until the job stops and its output closes, or until `timeout` ends.
    ///
    /// Processes that the job starts in the background can keep the output open, so the wait
    /// ends at most 250 ms after the job stops.
    pub(super) fn wait(
        &self,
        timeout: Duration,
        cancellation: &CommandCancellation,
    ) -> Result<(), CommandError> {
        let deadline = Instant::now() + timeout;
        let mut state = self.state();
        loop {
            let now = Instant::now();
            let drained = state.output_closed
                || state
                    .finished
                    .is_some_and(|finished| now >= finished + Duration::from_millis(250));
            if (state.status != CommandJobStatus::Running && drained) || now >= deadline {
                return Ok(());
            }
            if cancellation.is_cancelled() {
                return Err(CommandError::Cancelled);
            }
            let pause = (deadline - now).min(Duration::from_millis(10));
            state = self
                .changed
                .wait_timeout(state, pause)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    pub(super) fn snapshot(&self, offset: u64, strip: bool) -> CommandJobSnapshot {
        let state = self.state();
        let read = state.output.read(offset, state.output_closed);
        let wall_time = state
            .finished
            .unwrap_or_else(Instant::now)
            .duration_since(self.started);
        CommandJobSnapshot {
            job_id: self.id,
            status: state.status,
            exit_code: state.exit_code,
            head: render(&read.head, strip),
            tail: render(&read.tail, strip),
            omitted_bytes: read.omitted_bytes,
            next_offset: read.next_offset,
            truncated_before: read.truncated_before,
            wall_time_ms: wall_time.as_millis().try_into().unwrap_or(u64::MAX),
        }
    }

    pub(super) fn is_running(&self) -> bool {
        self.state().status == CommandJobStatus::Running
    }

    pub(super) fn expired(&self, retention: Duration, now: Instant) -> bool {
        self.state()
            .finished
            .is_some_and(|finished| now.saturating_duration_since(finished) >= retention)
    }

    fn mark(&self, status: CommandJobStatus, exit_code: Option<i32>) {
        let mut state = self.state();
        if state.status != CommandJobStatus::Running {
            return;
        }
        state.status = status;
        state.exit_code = exit_code;
        state.finished = Some(Instant::now());
        self.changed.notify_all();
    }

    fn report<M>(&self, message: M)
    where
        M: std::fmt::Display,
    {
        let message = format!("\novid-native: {message}\n");
        self.state().output.push(message.as_bytes());
    }

    fn require_running(&self) -> Result<(), CommandError> {
        self.is_running()
            .then_some(())
            .ok_or_else(|| CommandError::Execution("command job is not running".to_owned()))
    }

    fn state(&self) -> MutexGuard<'_, JobState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn stdin(&self) -> MutexGuard<'_, Option<Box<dyn Write + Send>>> {
        self.stdin.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn terminal(&self) -> MutexGuard<'_, Option<Terminal>> {
        self.terminal.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
