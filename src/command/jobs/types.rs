use std::time::Duration;

use ovid_command_process::TerminalSize;

use crate::command::CommandError;
use crate::command::types::validate_input;

/// The first output bytes that every job keeps.
pub(super) const JOB_HEAD_BYTES: usize = 65_536;

#[derive(Clone, Debug)]
pub(crate) struct CommandJobRequest {
    pub command: String,
    pub cwd: Option<String>,
    pub env: Vec<(String, String)>,
    pub timeout: Option<Duration>,
    pub terminal: Option<TerminalSize>,
    pub max_buffer_bytes: usize,
    pub owner_tag: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub(crate) enum CommandJobStatus {
    Running,
    Exited,
    Failed,
    TimedOut,
    Killed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub(crate) enum CommandJobSignal {
    Interrupt,
    Terminate,
    Kill,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CommandJobSnapshot {
    pub job_id: u64,
    pub status: CommandJobStatus,
    pub exit_code: Option<i32>,
    pub head: String,
    pub tail: String,
    pub omitted_bytes: u64,
    pub next_offset: u64,
    pub truncated_before: u64,
    pub wall_time_ms: u64,
}

/// Limits of one job registry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CommandJobLimits {
    pub max_running: usize,
    pub buffer_limit: usize,
    pub retention: Duration,
}

impl Default for CommandJobLimits {
    fn default() -> Self {
        Self {
            max_running: 8,
            buffer_limit: 8 * 1_048_576,
            retention: Duration::from_secs(600),
        }
    }
}

impl CommandJobRequest {
    pub(super) fn validate(&self, limits: &CommandJobLimits) -> Result<(), CommandError> {
        validate_input(&self.command, &self.env)?;
        if self.timeout.is_some_and(|timeout| {
            timeout < Duration::from_secs(1) || timeout > Duration::from_secs(86_400)
        }) {
            return Err(CommandError::Configuration(
                "command job timeout must be between 1 and 86400 seconds".to_owned(),
            ));
        }
        if !(JOB_HEAD_BYTES..=limits.buffer_limit).contains(&self.max_buffer_bytes) {
            return Err(CommandError::Configuration(format!(
                "command job buffer must be between {JOB_HEAD_BYTES} and {} bytes",
                limits.buffer_limit
            )));
        }
        if self
            .terminal
            .is_some_and(|size| size.cols == 0 || size.rows == 0)
        {
            return Err(CommandError::Configuration(
                "command job terminal size must be positive".to_owned(),
            ));
        }
        Ok(())
    }
}

impl CommandJobStatus {
    #[must_use]
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Exited => "exited",
            Self::Failed => "failed",
            Self::TimedOut => "timed_out",
            Self::Killed => "killed",
        }
    }
}

impl CommandJobSignal {
    pub(crate) fn parse(value: &str) -> Result<Self, CommandError> {
        [
            ("interrupt", Self::Interrupt),
            ("terminate", Self::Terminate),
            ("kill", Self::Kill),
        ]
        .into_iter()
        .find_map(|(name, signal)| (name == value).then_some(signal))
        .ok_or_else(|| CommandError::Configuration(format!("unknown command job signal: {value}")))
    }
}
