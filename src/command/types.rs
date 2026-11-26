use std::time::Duration;

use crate::workspace::{Cancellation, WorkspaceError};

#[derive(Clone, Debug)]
pub(crate) struct CommandRequest {
    pub command: String,
    pub cwd: Option<String>,
    pub env: Vec<(String, String)>,
    pub timeout: Duration,
    pub max_output_bytes: usize,
    pub max_head_bytes: usize,
    pub cancellation: CommandCancellation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CommandResult {
    pub head: String,
    pub tail: String,
    pub omitted_bytes: u64,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
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

impl CommandRequest {
    pub(crate) fn validate(&self) -> Result<(), CommandError> {
        if self.command.contains('\0') {
            return Err(CommandError::Configuration(
                "command cannot contain NUL bytes".to_owned(),
            ));
        }
        if self.timeout < Duration::from_secs(1) || self.timeout > Duration::from_secs(3_600) {
            return Err(CommandError::Configuration(
                "command timeout must be between 1 and 3600 seconds".to_owned(),
            ));
        }
        if self.max_output_bytes == 0 {
            return Err(CommandError::Configuration(
                "command output limit must be positive".to_owned(),
            ));
        }
        if self.max_head_bytes > self.max_output_bytes {
            return Err(CommandError::Configuration(
                "command head limit cannot exceed the output limit".to_owned(),
            ));
        }
        for (name, value) in &self.env {
            if name.is_empty() || name.contains('=') || name.contains('\0') || value.contains('\0')
            {
                return Err(CommandError::Configuration(
                    "command environment contains an invalid entry".to_owned(),
                ));
            }
        }
        Ok(())
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
