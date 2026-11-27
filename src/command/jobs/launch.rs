use std::io::{self, PipeReader, PipeWriter};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Instant;

use brush_core::{SourceInfo, openfiles::OpenFile};
use ovid_command_process::{TerminalProcess, TerminalSize};
use tokio::runtime::Runtime;
use tokio::time;

use crate::command::jobs::CommandJobRequest;
use crate::command::jobs::job::CommandJob;
use crate::command::jobs::ring::OutputRing;
use crate::command::{
    COMMAND_RUNTIME, CommandError, create_shell, execution_parameters, exit_code,
    wait_for_cancellation,
};

/// How the first process or the shell of a job ended.
pub(super) enum JobEnd {
    Exited(i32),
    Failed(String),
    TimedOut,
    Stopped,
}

/// Starts a job and the threads that run it and read its output.
pub(super) fn launch(
    id: u64,
    request: CommandJobRequest,
    cwd: PathBuf,
    output: OutputRing,
) -> Result<Arc<CommandJob>, CommandError> {
    let runtime = COMMAND_RUNTIME
        .as_ref()
        .map_err(|error| CommandError::Execution(error.to_string()))?;
    let deadline = request.timeout.map(|timeout| Instant::now() + timeout);
    let job = Arc::new(CommandJob::new(id, output, deadline));
    match request.terminal {
        Some(size) => start_terminal(runtime, &job, &request, &cwd, size)?,
        None => start_shell(runtime, &job, request, cwd)?,
    }
    Ok(job)
}

fn start_terminal(
    runtime: &'static Runtime,
    job: &Arc<CommandJob>,
    request: &CommandJobRequest,
    cwd: &Path,
    size: TerminalSize,
) -> Result<(), CommandError> {
    let mut command = shell_command(&request.command)?;
    command
        .current_dir(cwd)
        .envs(request.env.iter().map(|(name, value)| (name, value)));
    let TerminalProcess {
        terminal,
        input,
        output,
        child,
    } = job
        .scope
        .spawn_terminal(command, size)
        .map_err(|error| CommandError::Execution(error.to_string()))?;
    job.attach(Box::new(input), Some(terminal));

    start_thread(job, move |job| job.read_output(output))?;
    start_thread(job, move |job| {
        let exited = async move {
            match tokio::task::spawn_blocking(move || child.wait()).await {
                Ok(Ok(code)) => JobEnd::Exited(code),
                Ok(Err(error)) => JobEnd::Failed(error.to_string()),
                Err(error) => JobEnd::Failed(error.to_string()),
            }
        };
        job.finish(runtime.block_on(supervise(job, exited)));
    })
}

fn start_shell(
    runtime: &'static Runtime,
    job: &Arc<CommandJob>,
    request: CommandJobRequest,
    cwd: PathBuf,
) -> Result<(), CommandError> {
    let (stdin, input) = io::pipe().map_err(output_error)?;
    let (output, stdout) = io::pipe().map_err(output_error)?;
    let stderr = stdout.try_clone().map_err(output_error)?;
    job.attach(Box::new(input), None);

    start_thread(job, move |job| job.read_output(output))?;
    start_thread(job, move |job| {
        let end = runtime.block_on(run_shell(job, request, cwd, stdin, stdout, stderr));
        job.finish(end);
    })
}

fn start_thread<F>(job: &Arc<CommandJob>, work: F) -> Result<(), CommandError>
where
    F: FnOnce(&CommandJob) + Send + 'static,
{
    let job = Arc::clone(job);
    std::thread::Builder::new()
        .name(format!("ovid-command-job-{}", job.id))
        .spawn(move || work(&job))
        .map(drop)
        .map_err(|error| CommandError::Execution(error.to_string()))
}

async fn run_shell(
    job: &CommandJob,
    request: CommandJobRequest,
    cwd: PathBuf,
    stdin: PipeReader,
    stdout: PipeWriter,
    stderr: PipeWriter,
) -> JobEnd {
    let mut shell = match create_shell(&cwd, &request.env, Arc::clone(&job.scope)).await {
        Ok(shell) => shell,
        Err(error) => return JobEnd::Failed(error.to_string()),
    };
    let params = execution_parameters(&shell, OpenFile::from(stdin), stdout, stderr);
    let source = SourceInfo::from("ovid-native:command-job");

    let end = {
        let execution = async {
            match shell.run_string(request.command, &source, &params).await {
                Ok(result) => JobEnd::Exited(exit_code(&result)),
                Err(error) => JobEnd::Failed(error.to_string()),
            }
        };
        supervise(job, execution).await
    };
    // The job keeps its scope, so processes that the command starts in the background still run.
    drop((params, shell));
    end
}

async fn supervise<Fut>(job: &CommandJob, completion: Fut) -> JobEnd
where
    Fut: Future<Output = JobEnd>,
{
    let deadline = async {
        match job.deadline {
            Some(deadline) => time::sleep_until(deadline.into()).await,
            None => std::future::pending().await,
        }
    };
    tokio::select! {
        end = completion => end,
        () = deadline => JobEnd::TimedOut,
        () = wait_for_cancellation(&job.cancellation) => JobEnd::Stopped,
    }
}

#[cfg(windows)]
fn shell_command(command_text: &str) -> Result<Command, CommandError> {
    use std::os::windows::process::CommandExt;

    let shell = std::env::var_os("ComSpec").ok_or_else(|| {
        CommandError::Configuration("terminal jobs need the ComSpec variable".to_owned())
    })?;
    let mut command = Command::new(shell);
    command.raw_arg(format!("/d /s /c \"{command_text}\""));
    Ok(command)
}

#[cfg(unix)]
fn shell_command(command_text: &str) -> Result<Command, CommandError> {
    let mut command = Command::new("/bin/sh");
    command.arg("-c").arg(command_text);
    Ok(command)
}

fn output_error(error: io::Error) -> CommandError {
    CommandError::Output(error.to_string())
}
