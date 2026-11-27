use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::os::windows::process::CommandExt;
use std::process::Command;
use std::ptr;
use std::time::Duration;

use tokio::process::Child;
use windows_sys::Win32::Foundation::{HANDLE, NTSTATUS};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject,
};
use windows_sys::Win32::System::Threading::CREATE_SUSPENDED;

#[derive(Default)]
pub(crate) struct Members {
    job: Option<OwnedHandle>,
}

pub(crate) fn spawn(
    members: &mut Members,
    mut command: Command,
    kill_on_drop: bool,
) -> io::Result<(Child, Option<i32>)> {
    let job = match members.job.take() {
        Some(job) => job,
        None => create_job()?,
    };
    let job = members.job.insert(job);
    // A suspended child cannot start a grandchild before the job contains it.
    command.creation_flags(CREATE_SUSPENDED);
    let mut command = tokio::process::Command::from(command);
    command.kill_on_drop(kill_on_drop);
    let mut child = command.spawn()?;

    if let Err(error) = adopt(job, &child) {
        let _ = child.start_kill();
        return Err(error);
    }
    Ok((child, None))
}

pub(crate) fn terminate(members: &mut Members, _grace: Duration) -> io::Result<()> {
    let Some(job) = &members.job else {
        return Ok(());
    };
    // SAFETY: The job handle is open and has full access.
    if unsafe { TerminateJobObject(job.as_raw_handle(), 1) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[link(name = "ntdll.dll", kind = "raw-dylib", modifiers = "+verbatim")]
unsafe extern "system" {
    fn NtResumeProcess(process: HANDLE) -> NTSTATUS;
}

fn create_job() -> io::Result<OwnedHandle> {
    // SAFETY: Null attributes and a null name create an unnamed job with default security.
    let raw = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
    if raw.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: CreateJobObjectW returned a new handle that nothing else owns.
    let job = unsafe { OwnedHandle::from_raw_handle(raw) };
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    let size = u32::try_from(size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>())
        .map_err(io::Error::other)?;
    // SAFETY: The pointer and size describe `limits`, which lives for the whole call.
    let applied = unsafe {
        SetInformationJobObject(
            job.as_raw_handle(),
            JobObjectExtendedLimitInformation,
            (&raw const limits).cast(),
            size,
        )
    };
    if applied == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(job)
}

fn adopt(job: &OwnedHandle, child: &Child) -> io::Result<()> {
    let process = child
        .raw_handle()
        .ok_or_else(|| io::Error::other("spawned process has no handle"))?;
    // SAFETY: Both handles are open. CreateProcessW gave the child handle full access.
    if unsafe { AssignProcessToJobObject(job.as_raw_handle(), process) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: The child handle is open and has PROCESS_SUSPEND_RESUME access.
    let status = unsafe { NtResumeProcess(process) };
    if status < 0 {
        return Err(io::Error::other(format!(
            "cannot resume the spawned process: NTSTATUS {status:#010x}"
        )));
    }
    Ok(())
}
