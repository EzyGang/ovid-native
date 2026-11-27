use std::fs::File;
use std::io;
use std::os::windows::io::{AsRawHandle, OwnedHandle};
use std::process::Command;

use windows_sys::Win32::Foundation::WAIT_FAILED;
use windows_sys::Win32::System::Console::{
    COORD, ClosePseudoConsole, CreatePseudoConsole, HPCON, ResizePseudoConsole,
};
use windows_sys::Win32::System::Threading::{GetExitCodeProcess, INFINITE, WaitForSingleObject};

use crate::windows_process::create_process;
use crate::{Terminal, TerminalChild, TerminalOutput, TerminalProcess, TerminalSize};

pub(crate) struct Console {
    handle: HPCON,
}

pub(crate) struct Process {
    handle: OwnedHandle,
}

/// Starts `command` suspended on a new pseudoconsole, puts it in `job`, then resumes it.
///
/// The command line is the quoted program followed by each argument exactly as given, as
/// `CommandExt::raw_arg` passes it.
pub(crate) fn spawn(
    job: &OwnedHandle,
    command: &Command,
    size: TerminalSize,
) -> io::Result<TerminalProcess> {
    let (input_read, input_write) = io::pipe()?;
    let (output_read, output_write) = io::pipe()?;
    let mut handle: HPCON = 0;
    // SAFETY: Both pipe handles are open and `handle` is a valid out pointer.
    let created = unsafe {
        CreatePseudoConsole(
            coord(size)?,
            input_read.as_raw_handle(),
            output_write.as_raw_handle(),
            0,
            &raw mut handle,
        )
    };
    if created < 0 {
        return Err(io::Error::other(format!(
            "cannot create a pseudoconsole: HRESULT {created:#010x}"
        )));
    }
    let console = Console { handle };
    let process = Process {
        handle: create_process(job, command, console.handle)?,
    };
    // The pseudoconsole keeps its own copies of these ends.
    drop((input_read, output_write));

    Ok(TerminalProcess {
        terminal: Terminal { inner: console },
        input: File::from(OwnedHandle::from(input_write)),
        output: TerminalOutput {
            file: File::from(OwnedHandle::from(output_read)),
        },
        child: TerminalChild { inner: process },
    })
}

impl Console {
    pub(crate) fn resize(&self, size: TerminalSize) -> io::Result<()> {
        // SAFETY: The handle is an open pseudoconsole.
        let resized = unsafe { ResizePseudoConsole(self.handle, coord(size)?) };
        if resized < 0 {
            return Err(io::Error::other(format!(
                "cannot resize the pseudoconsole: HRESULT {resized:#010x}"
            )));
        }
        Ok(())
    }
}

impl Drop for Console {
    fn drop(&mut self) {
        // SAFETY: The handle is an open pseudoconsole that only this value closes.
        unsafe { ClosePseudoConsole(self.handle) };
    }
}

impl Process {
    pub(crate) fn wait(self) -> io::Result<i32> {
        let raw = self.handle.as_raw_handle();
        // SAFETY: The process handle is open and has SYNCHRONIZE access.
        if unsafe { WaitForSingleObject(raw, INFINITE) } == WAIT_FAILED {
            return Err(io::Error::last_os_error());
        }
        let mut code = 0u32;
        // SAFETY: The handle is open and `code` is a valid out pointer.
        if unsafe { GetExitCodeProcess(raw, &raw mut code) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(i32::from_ne_bytes(code.to_ne_bytes()))
    }
}

fn coord(size: TerminalSize) -> io::Result<COORD> {
    Ok(COORD {
        X: i16::try_from(size.cols).map_err(io::Error::other)?,
        Y: i16::try_from(size.rows).map_err(io::Error::other)?,
    })
}
