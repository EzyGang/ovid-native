use std::collections::HashMap;
use std::ptr;

use libc::{c_int, pid_t};

use crate::ProcessIdentity;
use crate::unix_orphans::Inspected;

pub(crate) fn inspect(pid: u32) -> Option<Inspected> {
    let process = c_int::try_from(pid).ok()?;
    let size = c_int::try_from(size_of::<libc::proc_bsdinfo>()).ok()?;
    // SAFETY: proc_bsdinfo holds only integers and arrays of integers, so zero bytes are valid.
    let mut info = unsafe { std::mem::zeroed::<libc::proc_bsdinfo>() };
    // SAFETY: The pointer and size describe `info`, which lives for the whole call.
    let read = unsafe {
        libc::proc_pidinfo(
            process,
            libc::PROC_PIDTBSDINFO,
            0,
            (&raw mut info).cast(),
            size,
        )
    };
    if read != size || info.pbi_status == libc::SZOMB {
        return None;
    }
    Some(Inspected {
        identity: ProcessIdentity {
            pid,
            start_time: info
                .pbi_start_tvsec
                .saturating_mul(1_000_000)
                .saturating_add(info.pbi_start_tvusec),
        },
        group: i32::try_from(info.pbi_pgid).ok()?,
    })
}

pub(crate) fn group_members(groups: &[i32]) -> HashMap<i32, Vec<ProcessIdentity>> {
    let mut members = HashMap::<i32, Vec<ProcessIdentity>>::new();
    for &group in groups {
        let identities = list(group)
            .into_iter()
            .filter_map(inspect)
            .filter(|process| process.group == group)
            .map(|process| process.identity);
        members.entry(group).or_default().extend(identities);
    }
    members
}

pub(crate) const fn boot_id() -> Option<String> {
    None
}

fn list(group: i32) -> Vec<u32> {
    // SAFETY: A null buffer asks only for the size that the list needs.
    let needed = unsafe { libc::proc_listpgrppids(group, ptr::null_mut(), 0) };
    let Ok(needed) = usize::try_from(needed) else {
        return Vec::new();
    };
    // The group can grow between the two calls.
    let mut pids = vec![pid_t::default(); needed / size_of::<pid_t>() + 16];
    let Ok(capacity) = c_int::try_from(pids.len() * size_of::<pid_t>()) else {
        return Vec::new();
    };
    // SAFETY: The pointer and size describe `pids`, which lives for the whole call.
    let read = unsafe { libc::proc_listpgrppids(group, pids.as_mut_ptr().cast(), capacity) };
    pids.truncate(usize::try_from(read).unwrap_or(0) / size_of::<pid_t>());
    pids.into_iter()
        .filter_map(|pid| u32::try_from(pid).ok())
        .filter(|pid| *pid > 0)
        .collect()
}
