use std::time::Duration;

use brush_core::{ExecutionExitCode, ExecutionResult};

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
    pub owner_tag: Option<String>,
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
        validate_input(&self.command, &self.env)?;
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
        Ok(())
    }
}

/// Checks the command text and environment that every command and command job share.
pub(crate) fn validate_input(command: &str, env: &[(String, String)]) -> Result<(), CommandError> {
    if command.contains('\0') {
        return Err(CommandError::Configuration(
            "command cannot contain NUL bytes".to_owned(),
        ));
    }
    for (name, value) in env {
        if name.is_empty() || name.contains('=') || name.contains('\0') || value.contains('\0') {
            return Err(CommandError::Configuration(
                "command environment contains an invalid entry".to_owned(),
            ));
        }
    }
    Ok(())
}

pub(crate) const fn exit_code(result: &ExecutionResult) -> i32 {
    match result.exit_code {
        ExecutionExitCode::Success => 0,
        ExecutionExitCode::GeneralError => 1,
        ExecutionExitCode::InvalidUsage => 2,
        ExecutionExitCode::Unimplemented => 99,
        ExecutionExitCode::CannotExecute => 126,
        ExecutionExitCode::NotFound => 127,
        ExecutionExitCode::Interrupted => 130,
        ExecutionExitCode::BrokenPipe => 141,
        ExecutionExitCode::Custom(code) => code as i32,
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
    JobLimit(String),
    JobNotFound(u64),
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

impl std::fmt::Display for CommandError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Configuration(message)
            | Self::Path(message)
            | Self::Execution(message)
            | Self::Output(message)
            | Self::JobLimit(message) => formatter.write_str(message),
            Self::Cancelled => formatter.write_str("command execution cancelled"),
            Self::Closed => formatter.write_str("workspace session is closed"),
            Self::JobNotFound(id) => write!(formatter, "command job {id} does not exist"),
        }
    }
}
