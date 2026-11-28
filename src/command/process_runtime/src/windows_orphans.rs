use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::time::Duration;

use windows_sys::Win32::Foundation::{FILETIME, WAIT_TIMEOUT};
use windows_sys::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, PROCESS_ACCESS_RIGHTS, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_SYNCHRONIZE, PROCESS_TERMINATE, TerminateProcess, WaitForSingleObject,
};

use crate::{ProcessIdentity, ProcessScope, ScopeMembers, windows};

pub(crate) fn identity(pid: u32) -> Option<ProcessIdentity> {
    let process = open(pid, PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE)?;
    live_identity(pid, &process)
}

pub(crate) const fn boot_id() -> Option<String> {
    None
}

pub(crate) fn sample(scopes: &[&ProcessScope]) -> Vec<ScopeMembers> {
    scopes
        .iter()
        .map(|scope| ScopeMembers {
            group: None,
            members: scope
                .read_members(windows::job_pids)
                .and_then(Result::ok)
                .unwrap_or_default()
                .into_iter()
                .filter_map(identity)
                .collect(),
        })
        .collect()
}

pub(crate) fn stop(
    _group: Option<i32>,
    members: &[ProcessIdentity],
    _grace: Duration,
) -> Vec<ProcessIdentity> {
    members
        .iter()
        .copied()
        .filter(|member| terminate(*member))
        .collect()
}

fn terminate(member: ProcessIdentity) -> bool {
    let access = PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE;
    let Some(process) = open(member.pid, access) else {
        return false;
    };
    // The open handle keeps the process ID, so the check and the termination reach one process.
    // SAFETY: The handle is open and has PROCESS_TERMINATE access.
    live_identity(member.pid, &process) == Some(member)
        && unsafe { TerminateProcess(process.as_raw_handle(), 1) } != 0
}

fn open(pid: u32, access: PROCESS_ACCESS_RIGHTS) -> Option<OwnedHandle> {
    // SAFETY: OpenProcess takes plain values and returns a new handle or null.
    let raw = unsafe { OpenProcess(access, 0, pid) };
    // SAFETY: A non-null handle from OpenProcess is new, and nothing else owns it.
    (!raw.is_null()).then(|| unsafe { OwnedHandle::from_raw_handle(raw) })
}

fn live_identity(pid: u32, process: &OwnedHandle) -> Option<ProcessIdentity> {
    // An exited process stays open while a handle to it remains.
    // SAFETY: The handle is open and has SYNCHRONIZE access.
    if unsafe { WaitForSingleObject(process.as_raw_handle(), 0) } != WAIT_TIMEOUT {
        return None;
    }
    let mut created = FILETIME::default();
    let mut exited = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: The handle is open, and each pointer refers to a live FILETIME.
    let read = unsafe {
        GetProcessTimes(
            process.as_raw_handle(),
            &raw mut created,
            &raw mut exited,
            &raw mut kernel,
            &raw mut user,
        )
    };
    (read != 0).then(|| ProcessIdentity {
        pid,
        start_time: (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime),
    })
}
