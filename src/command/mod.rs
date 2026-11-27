mod launcher;
mod output;
mod python;
#[cfg(test)]
mod scope_tests;
#[cfg(test)]
mod tests;
mod types;

use std::io::{PipeReader, PipeWriter};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use brush_builtins::{BuiltinSet, default_builtins};
use brush_core::{
    ExecutionExitCode, ExecutionParameters, ExecutionResult, ProfileLoadBehavior, RcLoadBehavior,
    Shell, ShellValue, ShellVariable, SourceInfo,
    extensions::{DefaultErrorFormatter, ShellExtensionsImpl},
    openfiles::{self, OpenFile, OpenFiles},
};
use ovid_command_process::ProcessScope;
use tokio::runtime::Runtime;
use tokio::time;

use crate::command::launcher::ScopedLauncher;
use crate::command::output::{CapturedOutput, capture};
use crate::workspace::Workspace;

pub(crate) use python::register;
pub(crate) use types::{CommandCancellation, CommandError, CommandRequest, CommandResult};

pub(crate) fn execute(
    workspace: &Workspace,
    request: CommandRequest,
) -> Result<CommandResult, CommandError> {
    request.validate()?;
    workspace
        .ensure_open()
        .map_err(CommandError::from_workspace)?;
    let cwd = resolve_cwd(workspace.root(), request.cwd.as_deref())?;
    let (reader, writer) =
        std::io::pipe().map_err(|error| CommandError::Output(error.to_string()))?;
    let stderr = writer
        .try_clone()
        .map_err(|error| CommandError::Output(error.to_string()))?;
    let runtime = COMMAND_RUNTIME
        .as_ref()
        .map_err(|error| CommandError::Execution(error.to_string()))?;
    let started = std::time::Instant::now();
    let (outcome, output) =
        runtime.block_on(execute_async(&request, &cwd, reader, writer, stderr))?;

    Ok(CommandResult {
        head: output.head,
        tail: output.tail,
        omitted_bytes: output.omitted_bytes,
        exit_code: outcome.result.as_ref().map(exit_code),
        timed_out: outcome.timed_out,
        wall_time_ms: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
    })
}

type CommandShell = Shell<ShellExtensionsImpl<DefaultErrorFormatter, ScopedLauncher>>;

static COMMAND_RUNTIME: LazyLock<std::io::Result<Runtime>> = LazyLock::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .thread_name("ovid-command")
        .enable_all()
        .build()
});

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
) -> Result<(CommandOutcome, CapturedOutput), CommandError> {
    let head_limit = request.max_head_bytes;
    let output_limit = request.max_output_bytes;
    let output_task =
        tokio::task::spawn_blocking(move || capture(reader, head_limit, output_limit));
    let outcome = run(request, cwd, stdout, stderr).await;
    let output = output_task
        .await
        .map_err(|error| CommandError::Output(error.to_string()))??;
    Ok((outcome?, output))
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
    let scope = Arc::new(ProcessScope::default());
    let mut shell = create_shell(cwd, &request.env, Arc::clone(&scope)).await?;
    let params = execution_parameters(&shell, stdout, stderr)?;
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
    // Dropped child handles let the runtime reap the processes that termination stops.
    drop((params, shell));
    tokio::task::spawn_blocking(move || scope.terminate(Duration::from_millis(500)))
        .await
        .map_err(|error| CommandError::Execution(error.to_string()))?
        .map_err(|error| CommandError::Execution(error.to_string()))?;

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

async fn create_shell(
    cwd: &Path,
    env: &[(String, String)],
    scope: Arc<ProcessScope>,
) -> Result<CommandShell, CommandError> {
    let mut shell = Shell::builder_with_extensions()
        .do_not_inherit_env(true)
        .profile(ProfileLoadBehavior::Skip)
        .rc(RcLoadBehavior::Skip)
        .builtins(default_builtins(BuiltinSet::BashMode))
        .process_launcher(ScopedLauncher { scope })
        .working_dir(cwd.to_path_buf())
        .build()
        .await
        .map_err(|error| CommandError::Execution(error.to_string()))?;
    for name in ["exec", "suspend"] {
        if let Some(builtin) = shell.builtins_mut().get_mut(name) {
            builtin.set_enabled(false);
        }
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
    shell: &CommandShell,
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
