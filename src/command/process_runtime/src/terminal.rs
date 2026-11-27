use std::fs::File;
use std::io::{self, Read};

#[cfg(unix)]
use crate::unix_terminal as platform;
#[cfg(windows)]
use crate::windows_terminal as platform;

/// Size of a pseudoterminal in character cells.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct TerminalSize {
    pub cols: u16,
    pub rows: u16,
}

/// A process that runs attached to its own pseudoterminal.
///
/// `input` sends bytes to the terminal and `output` reads everything that the terminal shows.
/// `output` reaches its end when no process holds the terminal any longer. On Windows, that
/// happens only after `terminal` drops, so drop `terminal` after `child` exits.
pub struct TerminalProcess {
    pub terminal: Terminal,
    pub input: File,
    pub output: TerminalOutput,
    pub child: TerminalChild,
}

/// The controlling side of a pseudoterminal.
pub struct Terminal {
    pub(crate) inner: platform::Console,
}

/// Reads the bytes that a pseudoterminal shows.
pub struct TerminalOutput {
    pub(crate) file: File,
}

/// The first process of a pseudoterminal.
pub struct TerminalChild {
    pub(crate) inner: platform::Process,
}

impl Terminal {
    /// Changes the size that the processes of the terminal see.
    pub fn resize(&self, size: TerminalSize) -> io::Result<()> {
        self.inner.resize(size)
    }
}

impl Read for TerminalOutput {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        match self.file.read(buffer) {
            Err(error) if is_closed(&error) => Ok(0),
            result => result,
        }
    }
}

impl TerminalChild {
    /// Blocks until the process exits and returns its exit code.
    ///
    /// On Unix, a process that a signal stops returns 128 plus the signal number.
    pub fn wait(self) -> io::Result<i32> {
        self.inner.wait()
    }
}

fn is_closed(error: &io::Error) -> bool {
    #[cfg(unix)]
    if error.raw_os_error() == Some(libc::EIO) {
        return true;
    }
    error.kind() == io::ErrorKind::BrokenPipe
}
