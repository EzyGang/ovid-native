mod processes;
mod python;
#[cfg(test)]
mod tests;
mod types;

use std::collections::VecDeque;
use std::io::{PipeReader, PipeWriter, Read};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use brush_builtins::{BuiltinSet, default_builtins};
use brush_core::{
    ExecutionExitCode, ExecutionParameters, ExecutionResult, ProcessGroupPolicy,
    ProfileLoadBehavior, RcLoadBehavior, Shell as BrushShell, ShellValue, ShellVariable,
    SourceInfo,
    openfiles::{self, OpenFile, OpenFiles},
    traps::TrapSignal,
};
use tokio::time;

use crate::command::processes::SpawnRegistry;
use crate::workspace::Workspace;

pub(crate) use python::register;
pub(crate) use types::{CommandCancellation, CommandError, CommandRequest, CommandResult};

static COMMAND_EXECUTION_LOCK: Mutex<()> = Mutex::new(());

pub(crate) fn execute(
    workspace: &Workspace,
    request: CommandRequest,
) -> Result<CommandResult, CommandError> {
    validate(&request)?;
    workspace
        .ensure_open()
        .map_err(CommandError::from_workspace)?;
    let _execution_guard = COMMAND_EXECUTION_LOCK
        .lock()
        .map_err(|_| CommandError::Execution("command execution lock is unavailable".to_owned()))?;
    let cwd = resolve_cwd(workspace.root(), request.cwd.as_deref())?;
    let (reader, writer) =
        std::io::pipe().map_err(|error| CommandError::Output(error.to_string()))?;
    let stderr = writer
        .try_clone()
        .map_err(|error| CommandError::Output(error.to_string()))?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| CommandError::Execution(error.to_string()))?;
    let started = std::time::Instant::now();
    let (outcome, output, truncated) =
        runtime.block_on(execute_async(&request, &cwd, reader, writer, stderr))?;

    Ok(CommandResult {
        output,
        exit_code: outcome.result.as_ref().map(exit_code),
        timed_out: outcome.timed_out,
        truncated,
        wall_time_ms: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
    })
}

struct CommandOutcome {
    result: Option<ExecutionResult>,
    timed_out: bool,
}

enum RunOutcome {
    Completed(Result<ExecutionResult, brush_core::Error>),
    TimedOut,
    Cancelled,
}

async fn execute_async(
    request: &CommandRequest,
    cwd: &Path,
    reader: PipeReader,
    stdout: PipeWriter,
    stderr: PipeWriter,
) -> Result<(CommandOutcome, String, bool), CommandError> {
    let output_limit = request.max_output_bytes;
    let output_task = tokio::task::spawn_blocking(move || read_tail(reader, output_limit));
    let outcome = run(request, cwd, stdout, stderr).await;
    let output = output_task
        .await
        .map_err(|error| CommandError::Output(error.to_string()))??;
    let outcome = outcome?;
    Ok((outcome, output.0, output.1))
}

fn validate(request: &CommandRequest) -> Result<(), CommandError> {
    if request.command.contains('\0') {
        return Err(CommandError::Configuration(
            "command cannot contain NUL bytes".to_owned(),
        ));
    }
    if request.timeout < Duration::from_secs(1) || request.timeout > Duration::from_secs(3_600) {
        return Err(CommandError::Configuration(
            "command timeout must be between 1 and 3600 seconds".to_owned(),
        ));
    }
    if request.max_output_bytes == 0 {
        return Err(CommandError::Configuration(
            "command output limit must be positive".to_owned(),
        ));
    }
    for (name, value) in &request.env {
        if name.is_empty() || name.contains('=') || name.contains('\0') || value.contains('\0') {
            return Err(CommandError::Configuration(
                "command environment contains an invalid entry".to_owned(),
            ));
        }
    }
    Ok(())
}

fn resolve_cwd(root: &Path, requested: Option<&str>) -> Result<PathBuf, CommandError> {
    let requested = Path::new(requested.unwrap_or("."));
    let candidate = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        root.join(requested)
    };
    let resolved = candidate
        .canonicalize()
        .map_err(|error| CommandError::Path(format!("cannot resolve command cwd: {error}")))?;
    if !resolved.starts_with(root) {
        return Err(CommandError::Path(
            "command cwd resolves outside the workspace".to_owned(),
        ));
    }
    if !resolved.is_dir() {
        return Err(CommandError::Path(
            "command cwd is not a directory".to_owned(),
        ));
    }
    Ok(dunce::simplified(&resolved).to_path_buf())
}

async fn run(
    request: &CommandRequest,
    cwd: &Path,
    stdout: PipeWriter,
    stderr: PipeWriter,
) -> Result<CommandOutcome, CommandError> {
    let spawns = Arc::new(SpawnRegistry::new());
    let mut shell = create_shell(cwd, &request.env).await?;
    let mut params = execution_parameters(&shell, stdout, stderr)?;
    params.process_group_policy = ProcessGroupPolicy::NewProcessGroup;
    let source = SourceInfo::from("ovid-native:command");

    let outcome = {
        let execution = shell.run_string(request.command.clone(), &source, &params);
        tokio::pin!(execution);
        tokio::select! {
            result = &mut execution => RunOutcome::Completed(result),
            () = wait_for_cancellation(&request.cancellation) => RunOutcome::Cancelled,
            () = time::sleep(request.timeout) => RunOutcome::TimedOut,
        }
    };
    drop(params);
    spawns.terminate().await;
    terminate_jobs(&mut shell).await;

    match outcome {
        RunOutcome::Completed(result) => Ok(CommandOutcome {
            result: Some(result.map_err(|error| CommandError::Execution(error.to_string()))?),
            timed_out: false,
        }),
        RunOutcome::TimedOut => Ok(CommandOutcome {
            result: None,
            timed_out: true,
        }),
        RunOutcome::Cancelled => Err(CommandError::Cancelled),
    }
}

async fn create_shell(cwd: &Path, env: &[(String, String)]) -> Result<BrushShell, CommandError> {
    let mut shell = BrushShell::builder()
        .do_not_inherit_env(true)
        .profile(ProfileLoadBehavior::Skip)
        .rc(RcLoadBehavior::Skip)
        .builtins(default_builtins(BuiltinSet::BashMode))
        .working_dir(cwd.to_path_buf())
        .build()
        .await
        .map_err(|error| CommandError::Execution(error.to_string()))?;
    if let Some(exec_builtin) = shell.builtin_mut("exec") {
        exec_builtin.disabled = true;
    }
    if let Some(suspend_builtin) = shell.builtin_mut("suspend") {
        suspend_builtin.disabled = true;
    }
    for (name, value) in std::env::vars().chain(env.iter().cloned()) {
        let mut variable = ShellVariable::new(ShellValue::String(value));
        variable.export();
        shell
            .env_mut()
            .set_global(name, variable)
            .map_err(|error| CommandError::Execution(error.to_string()))?;
    }
    shell
        .set_working_dir(cwd)
        .map_err(|error| CommandError::Execution(error.to_string()))?;
    Ok(shell)
}

fn execution_parameters(
    shell: &BrushShell,
    stdout: PipeWriter,
    stderr: PipeWriter,
) -> Result<ExecutionParameters, CommandError> {
    let mut params = shell.default_exec_params();
    let stdin = openfiles::null().map_err(|error| CommandError::Output(error.to_string()))?;
    params.set_fd(OpenFiles::STDIN_FD, stdin);
    params.set_fd(OpenFiles::STDOUT_FD, OpenFile::from(stdout));
    params.set_fd(OpenFiles::STDERR_FD, OpenFile::from(stderr));
    Ok(params)
}

async fn wait_for_cancellation(cancellation: &CommandCancellation) {
    while !cancellation.is_cancelled() {
        time::sleep(Duration::from_millis(10)).await;
    }
}

async fn terminate_jobs(shell: &mut BrushShell) {
    let Ok(kill) = TrapSignal::try_from("KILL") else {
        return;
    };
    for job in &mut shell.jobs_mut().jobs {
        if job.representative_pid().is_some() {
            let _ = job.kill(kill);
            let _ = job.wait().await;
        }
    }
}

const fn exit_code(result: &ExecutionResult) -> i32 {
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

fn read_tail(mut reader: PipeReader, limit: usize) -> Result<(String, bool), CommandError> {
    let mut retained = VecDeque::with_capacity(limit);
    let mut total = 0u64;
    let mut chunk = [0u8; 65_536];
    loop {
        let read = reader
            .read(&mut chunk)
            .map_err(|error| CommandError::Output(error.to_string()))?;
        if read == 0 {
            break;
        }
        total = total.saturating_add(read.try_into().unwrap_or(u64::MAX));
        let bytes = &chunk[..read];
        if bytes.len() >= limit {
            retained.clear();
            retained.extend(&bytes[bytes.len() - limit..]);
            continue;
        }
        let overflow = retained
            .len()
            .saturating_add(bytes.len())
            .saturating_sub(limit);
        retained.drain(..overflow);
        retained.extend(bytes);
    }
    let bytes = retained.make_contiguous();
    Ok((
        String::from_utf8_lossy(bytes).into_owned(),
        total > limit.try_into().unwrap_or(u64::MAX),
    ))
}
