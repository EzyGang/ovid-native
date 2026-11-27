use std::ffi::{OsStr, OsString, c_void};
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::process::Command;
use std::ptr;

use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::System::Console::HPCON;
use windows_sys::Win32::System::JobObjects::AssignProcessToJobObject;
use windows_sys::Win32::System::Threading::{
    CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, CreateProcessW, DeleteProcThreadAttributeList,
    EXTENDED_STARTUPINFO_PRESENT, InitializeProcThreadAttributeList, LPPROC_THREAD_ATTRIBUTE_LIST,
    PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE, PROCESS_INFORMATION, ResumeThread, STARTF_USESTDHANDLES,
    STARTUPINFOEXW, TerminateProcess, UpdateProcThreadAttribute,
};

/// Starts `command` suspended on `console`, puts it in `job`, resumes it, and returns its
/// process handle.
pub(crate) fn create_process(
    job: &OwnedHandle,
    command: &Command,
    console: HPCON,
) -> io::Result<OwnedHandle> {
    let mut attributes = AttributeList::new(console)?;
    let mut startup = STARTUPINFOEXW::default();
    startup.StartupInfo.cb =
        u32::try_from(size_of::<STARTUPINFOEXW>()).map_err(io::Error::other)?;
    // Invalid standard handles stop the child from inheriting redirected host handles, so the
    // child uses the pseudoconsole.
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.StartupInfo.hStdInput = INVALID_HANDLE_VALUE;
    startup.StartupInfo.hStdOutput = INVALID_HANDLE_VALUE;
    startup.StartupInfo.hStdError = INVALID_HANDLE_VALUE;
    startup.lpAttributeList = attributes.as_mut_ptr();

    let (process, thread) = create_suspended(command, &startup)?;
    if let Err(error) = start_in_job(job, &process, &thread) {
        // SAFETY: The process handle is open and has full access.
        unsafe { TerminateProcess(process.as_raw_handle(), 1) };
        return Err(error);
    }
    Ok(process)
}

fn create_suspended(
    command: &Command,
    startup: &STARTUPINFOEXW,
) -> io::Result<(OwnedHandle, OwnedHandle)> {
    let mut command_line = command_line(command);
    let environment = environment_block(command);
    let directory = command.get_current_dir().map(|path| wide(path.as_os_str()));
    let mut information = PROCESS_INFORMATION::default();
    // SAFETY: Every pointer refers to a live NUL-terminated buffer or to a live structure.
    let created = unsafe {
        CreateProcessW(
            ptr::null(),
            command_line.as_mut_ptr(),
            ptr::null(),
            ptr::null(),
            0,
            EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT | CREATE_SUSPENDED,
            environment.as_ptr().cast(),
            directory
                .as_ref()
                .map_or(ptr::null(), |directory| directory.as_ptr()),
            &raw const startup.StartupInfo,
            &raw mut information,
        )
    };
    if created == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: CreateProcessW returned new handles that nothing else owns.
    Ok(unsafe {
        (
            OwnedHandle::from_raw_handle(information.hProcess),
            OwnedHandle::from_raw_handle(information.hThread),
        )
    })
}

fn start_in_job(job: &OwnedHandle, process: &OwnedHandle, thread: &OwnedHandle) -> io::Result<()> {
    // SAFETY: Both handles are open. CreateProcessW gave the process handle full access.
    if unsafe { AssignProcessToJobObject(job.as_raw_handle(), process.as_raw_handle()) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: The thread handle is open and has THREAD_SUSPEND_RESUME access.
    if unsafe { ResumeThread(thread.as_raw_handle()) } == u32::MAX {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn command_line(command: &Command) -> Vec<u16> {
    let mut line = vec![u16::from(b'"')];
    line.extend(command.get_program().encode_wide());
    line.push(u16::from(b'"'));
    for argument in command.get_args() {
        line.push(u16::from(b' '));
        line.extend(argument.encode_wide());
    }
    line.push(0);
    line
}

fn environment_block(command: &Command) -> Vec<u16> {
    let mut variables: Vec<(OsString, OsString)> = std::env::vars_os().collect();
    for (name, value) in command.get_envs() {
        variables.retain(|(existing, _)| !existing.eq_ignore_ascii_case(name));
        if let Some(value) = value {
            variables.push((name.to_owned(), value.to_owned()));
        }
    }
    let mut block = Vec::new();
    for (name, value) in &variables {
        block.extend(name.encode_wide());
        block.push(u16::from(b'='));
        block.extend(value.encode_wide());
        block.push(0);
    }
    block.extend([0, 0]);
    block
}

fn wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain([0]).collect()
}

struct AttributeList {
    buffer: Vec<usize>,
}

impl AttributeList {
    fn new(console: HPCON) -> io::Result<Self> {
        let mut size = 0usize;
        // SAFETY: A null list with a valid size pointer only queries the required size.
        unsafe { InitializeProcThreadAttributeList(ptr::null_mut(), 1, 0, &raw mut size) };
        let mut buffer = vec![0usize; size.div_ceil(size_of::<usize>())];
        let list: LPPROC_THREAD_ATTRIBUTE_LIST = buffer.as_mut_ptr().cast();
        // SAFETY: The buffer is at least `size` bytes and aligned for the list.
        if unsafe { InitializeProcThreadAttributeList(list, 1, 0, &raw mut size) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut attributes = Self { buffer };
        // SAFETY: The list is initialized. The attribute value is the pseudoconsole handle itself.
        let updated = unsafe {
            UpdateProcThreadAttribute(
                attributes.as_mut_ptr(),
                0,
                PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
                console as *const c_void,
                size_of::<HPCON>(),
                ptr::null_mut(),
                ptr::null(),
            )
        };
        if updated == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(attributes)
    }

    fn as_mut_ptr(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.buffer.as_mut_ptr().cast()
    }
}

impl Drop for AttributeList {
    fn drop(&mut self) {
        // SAFETY: The list was initialized and is deleted once.
        unsafe { DeleteProcThreadAttributeList(self.as_mut_ptr()) };
    }
}
