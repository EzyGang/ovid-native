use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::{Child, Command, Stdio};
use std::ptr;

use crate::{Terminal, TerminalChild, TerminalOutput, TerminalProcess, TerminalSize};

pub(crate) struct Console {
    master: OwnedFd,
}

pub(crate) struct Process {
    child: Child,
}

/// Starts `command` as the leader of a new session whose controlling terminal is a new
/// pseudoterminal, and returns the process with its process group ID.
pub(crate) fn spawn(
    mut command: Command,
    size: TerminalSize,
) -> io::Result<(TerminalProcess, i32)> {
    let (master, slave) = open(size)?;
    command
        .stdin(Stdio::from(slave.try_clone()?))
        .stdout(Stdio::from(slave.try_clone()?))
        .stderr(Stdio::from(slave));
    // SAFETY: The hook calls only async-signal-safe functions between fork and exec.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 || libc::ioctl(0, libc::TIOCSCTTY as _, 0) == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command.spawn()?;
    // The parent copies of the terminal end must close, so the output ends with the session.
    drop(command);
    let Ok(group) = i32::try_from(child.id()) else {
        let _ = child.kill();
        return Err(io::Error::other("spawned process has no valid process ID"));
    };

    let process = TerminalProcess {
        input: File::from(master.try_clone()?),
        output: TerminalOutput {
            file: File::from(master.try_clone()?),
        },
        terminal: Terminal {
            inner: Console { master },
        },
        child: TerminalChild {
            inner: Process { child },
        },
    };
    Ok((process, group))
}

impl Console {
    pub(crate) fn resize(&self, size: TerminalSize) -> io::Result<()> {
        let window = window(size);
        // SAFETY: The descriptor is an open terminal master and the pointer refers to `window`.
        if unsafe {
            libc::ioctl(
                self.master.as_raw_fd(),
                libc::TIOCSWINSZ as _,
                &raw const window,
            )
        } == -1
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

impl Process {
    pub(crate) fn wait(mut self) -> io::Result<i32> {
        let status = self.child.wait()?;
        Ok(status
            .code()
            .unwrap_or_else(|| 128 + status.signal().unwrap_or(0)))
    }
}

fn open(size: TerminalSize) -> io::Result<(OwnedFd, OwnedFd)> {
    let mut master = -1;
    let mut slave = -1;
    let mut window = window(size);
    // SAFETY: The out pointers are valid. A null name and null terminal settings are allowed.
    let opened = unsafe {
        libc::openpty(
            &raw mut master,
            &raw mut slave,
            ptr::null_mut(),
            ptr::null_mut(),
            &raw mut window,
        )
    };
    if opened == -1 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: openpty returned two new descriptors that nothing else owns.
    let (master, slave) = unsafe { (OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave)) };
    for descriptor in [&master, &slave] {
        // SAFETY: The descriptor is open. Other spawned processes must not inherit it.
        if unsafe { libc::fcntl(descriptor.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } == -1 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok((master, slave))
}

const fn window(size: TerminalSize) -> libc::winsize {
    libc::winsize {
        ws_row: size.rows,
        ws_col: size.cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    }
}
