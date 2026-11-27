use std::sync::Arc;

use brush_core::{
    ExecutionResult,
    extensions::ProcessLauncher,
    processes::{ChildProcess, LaunchRequest, ProcessGroup},
    sys::tokio_process::compose_std_command,
};
use ovid_command_process::ProcessScope;

/// Launches every external process of one command inside that command's [`ProcessScope`].
#[derive(Clone, Default)]
pub(super) struct ScopedLauncher {
    pub scope: Arc<ProcessScope>,
}

impl ProcessLauncher for ScopedLauncher {
    fn spawn(&self, mut request: LaunchRequest) -> Result<ChildProcess, std::io::Error> {
        // The scope chooses the process group, so a request can never join another group.
        request.process_group = ProcessGroup::Inherit;
        let kill_on_drop = request.kill_on_drop;
        let (child, process_group) = self
            .scope
            .spawn(compose_std_command(request)?, kill_on_drop)?;
        let pid = child.id().and_then(|id| i32::try_from(id).ok());
        let exited = async move { child.wait_with_output().await.map(ExecutionResult::from) };
        Ok(ChildProcess::from_future(exited).native_child(pid, process_group))
    }
}
