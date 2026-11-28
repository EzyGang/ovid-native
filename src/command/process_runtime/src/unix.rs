use std::io;
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use libc::c_int;
use tokio::process::Child;

use crate::{TerminalProcess, TerminalSize, unix_terminal};

#[derive(Default)]
pub(crate) struct Members {
    group: Option<i32>,
}

pub(crate) fn spawn(
    members: &mut Members,
    mut command: Command,
    kill_on_drop: bool,
) -> io::Result<(Child, Option<i32>)> {
    // The first process leads a new group. A group with no live member cannot be joined.
    let joined = members.group.filter(|group| group_exists(*group));
    command.process_group(joined.unwrap_or(0));
    #[cfg(target_os = "linux")]
    let command = die_with_host(command);
    let mut command = tokio::process::Command::from(command);
    command.kill_on_drop(kill_on_drop);
    let mut child = command.spawn()?;

    let Some(group) = joined.or_else(|| child.id().and_then(|id| i32::try_from(id).ok())) else {
        let _ = child.start_kill();
        return Err(io::Error::other("spawned process has no process ID"));
    };
    members.group = Some(group);
    Ok((child, Some(group)))
}

pub(crate) fn spawn_terminal(
    members: &mut Members,
    command: Command,
    size: TerminalSize,
) -> io::Result<TerminalProcess> {
    // A session leader cannot join an existing process group.
    if members.group.is_some_and(group_exists) {
        return Err(io::Error::other(
            "a terminal process needs a scope without processes",
        ));
    }
    #[cfg(target_os = "linux")]
    let command = die_with_host(command);
    let (process, group) = unix_terminal::spawn(command, size)?;
    members.group = Some(group);
    Ok(process)
}

pub(crate) fn terminate(members: &mut Members, grace: Duration) -> io::Result<()> {
    let Some(group) = members.group.take() else {
        return Ok(());
    };
    signal(group, libc::SIGTERM)?;
    let deadline = Instant::now() + grace;
    while group_exists(group) {
        if Instant::now() >= deadline {
            return signal(group, libc::SIGKILL);
        }
        thread::sleep(Duration::from_millis(5));
    }
    Ok(())
}

pub(crate) const fn group(members: &Members) -> Option<i32> {
    members.group
}

pub(crate) fn stop_group(group: i32, grace: Duration) -> io::Result<()> {
    terminate(&mut Members { group: Some(group) }, grace)
}

/// Makes the child get `SIGKILL` when the spawning thread exits.
#[cfg(target_os = "linux")]
fn die_with_host(mut command: Command) -> Command {
    let host = std::process::id();
    // SAFETY: The hook calls only async-signal-safe functions between fork and exec.
    unsafe {
        command.pre_exec(move || {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL as libc::c_ulong) == -1 {
                return Err(io::Error::last_os_error());
            }
            // A host that dies before the signal is armed has already left the child an orphan.
            if u32::try_from(libc::getppid()).ok() != Some(host) {
                libc::_exit(1);
            }
            Ok(())
        });
    }
    command
}

impl Drop for Members {
    fn drop(&mut self) {
        if let Some(group) = self.group {
            let _ = signal(group, libc::SIGKILL);
        }
    }
}

fn signal(group: i32, signal: c_int) -> io::Result<()> {
    // SAFETY: getpgrp has no preconditions and cannot fail.
    if group <= 1 || group == unsafe { libc::getpgrp() } {
        return Err(io::Error::other(
            "process scope never signals the host process group",
        ));
    }
    // SAFETY: killpg takes plain integers. The group is one that this scope created.
    if unsafe { libc::killpg(group, signal) } == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    match error.raw_os_error() {
        Some(libc::ESRCH) => Ok(()),
        _ => Err(error),
    }
}

fn group_exists(group: i32) -> bool {
    // SAFETY: signal 0 only checks that a member of the group exists.
    group > 1 && unsafe { libc::killpg(group, 0) } == 0
}
