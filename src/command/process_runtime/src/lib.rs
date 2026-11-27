use std::io;
use std::process::Command;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use tokio::process::Child;

mod terminal;
#[cfg(unix)]
mod unix;
#[cfg(unix)]
mod unix_terminal;
#[cfg(unix)]
use unix as platform;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
mod windows_process;
#[cfg(windows)]
mod windows_terminal;
#[cfg(windows)]
use windows as platform;

pub use terminal::{Terminal, TerminalChild, TerminalOutput, TerminalProcess, TerminalSize};

/// Owns every process that one command starts.
///
/// On Windows, the scope is a Job Object. The scope closes the job when it drops, and the job then
/// stops every process in it. On Unix, the scope is one process group that the command owns. The
/// scope stops that group when it drops.
///
/// [`ProcessScope::spawn`] puts a child in the scope before the child runs its program. Each
/// descendant of the child stays in the scope, with these exceptions:
///
/// - On Unix, a process that calls `setsid` or `setpgid` leaves the process group.
/// - On Windows, a process that another service starts, for example through WMI or the Task
///   Scheduler, is not in the job. The job does not allow breakaway.
///
/// The scope contains only external processes. A builtin that the shell runs is not a separate
/// process, so the scope cannot stop it.
#[derive(Default)]
pub struct ProcessScope {
    state: Mutex<ScopeState>,
}

#[derive(Default)]
struct ScopeState {
    terminated: bool,
    members: platform::Members,
}

impl ProcessScope {
    /// Starts `command` inside the scope and returns the child with its process group ID.
    ///
    /// The process group ID is `None` on Windows. The scope refuses to start a process after
    /// [`ProcessScope::terminate`]. If the scope cannot contain the child, it stops the child and
    /// returns the error.
    pub fn spawn(&self, command: Command, kill_on_drop: bool) -> io::Result<(Child, Option<i32>)> {
        platform::spawn(&mut self.open_state()?.members, command, kill_on_drop)
    }

    /// Starts `command` attached to a new pseudoterminal inside the scope.
    ///
    /// The child joins the scope before it runs its program, so it cannot start a process outside
    /// the scope first. On Unix, the child leads a new session whose controlling terminal is the
    /// pseudoterminal, and the scope must not hold a process yet. On Windows, the command line is
    /// the quoted program followed by each argument exactly as given.
    pub fn spawn_terminal(
        &self,
        command: Command,
        size: TerminalSize,
    ) -> io::Result<TerminalProcess> {
        platform::spawn_terminal(&mut self.open_state()?.members, command, size)
    }

    /// Stops every process in the scope and refuses later spawns.
    ///
    /// On Unix, the scope sends `SIGTERM` to its process group, then `SIGKILL` if the group still
    /// exists after `grace`. On Windows, the scope stops the job at once. This call blocks the
    /// thread for at most `grace`.
    pub fn terminate(&self, grace: Duration) -> io::Result<()> {
        let mut state = self.state()?;
        state.terminated = true;
        platform::terminate(&mut state.members, grace)
    }

    fn open_state(&self) -> io::Result<MutexGuard<'_, ScopeState>> {
        let state = self.state()?;
        if state.terminated {
            return Err(io::Error::other("process scope is terminated"));
        }
        Ok(state)
    }

    fn state(&self) -> io::Result<MutexGuard<'_, ScopeState>> {
        self.state
            .lock()
            .map_err(|_| io::Error::other("process scope state is poisoned"))
    }
}
