use std::time::Duration;

use crate::workspace::{Cancellation, WorkspaceError};

#[derive(Clone, Debug)]
pub(crate) struct CommandRequest {
    pub command: String,
    pub cwd: Option<String>,
    pub env: Vec<(String, String)>,
    pub timeout: Duration,
    pub max_output_bytes: usize,
    pub cancellation: CommandCancellation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CommandResult {
    pub output: String,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub truncated: bool,
    pub wall_time_ms: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct CommandCancellation {
    inner: Cancellation,
}

impl CommandCancellation {
    pub(crate) fn new() -> Self {
        Self {
            inner: Cancellation::new(),
        }
    }

    pub(crate) fn cancel(&self) {
        self.inner.cancel();
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.inner.is_cancelled()
    }
}

#[derive(Debug)]
pub(crate) enum CommandError {
    Configuration(String),
    Path(String),
    Execution(String),
    Output(String),
    Cancelled,
    Closed,
}

impl CommandError {
    pub(crate) fn from_workspace(error: WorkspaceError) -> Self {
        match error {
            WorkspaceError::Closed => Self::Closed,
            error => {
                Self::Configuration(format!("workspace rejected command execution: {error:?}"))
            }
        }
    }
}
