use std::time::Duration;

#[cfg(target_os = "linux")]
use crate::linux_processes as processes;
#[cfg(target_os = "macos")]
use crate::macos_processes as processes;
use crate::{ProcessIdentity, ProcessScope, ScopeMembers, unix};

/// A live process and its process group.
pub(crate) struct Inspected {
    pub identity: ProcessIdentity,
    pub group: i32,
}

pub(crate) fn identity(pid: u32) -> Option<ProcessIdentity> {
    processes::inspect(pid).map(|process| process.identity)
}

pub(crate) fn boot_id() -> Option<String> {
    processes::boot_id()
}

pub(crate) fn sample(scopes: &[&ProcessScope]) -> Vec<ScopeMembers> {
    let groups = scopes
        .iter()
        .map(|scope| scope.read_members(unix::group).flatten())
        .collect::<Vec<_>>();
    let members = processes::group_members(&groups.iter().flatten().copied().collect::<Vec<_>>());
    groups
        .into_iter()
        .map(|group| ScopeMembers {
            group,
            members: group
                .and_then(|group| members.get(&group).cloned())
                .unwrap_or_default(),
        })
        .collect()
}

pub(crate) fn stop(
    group: Option<i32>,
    members: &[ProcessIdentity],
    grace: Duration,
) -> Vec<ProcessIdentity> {
    let matching = members
        .iter()
        .filter_map(|member| {
            processes::inspect(member.pid).filter(|process| process.identity == *member)
        })
        .collect::<Vec<_>>();
    // POSIX does not reuse a process group ID while the group has a member.
    let group = group.filter(|group| {
        matching.iter().any(|process| process.group == *group)
            && unix::stop_group(*group, grace).is_ok()
    });
    matching
        .into_iter()
        .filter(|process| Some(process.group) == group || kill(process.identity.pid))
        .map(|process| process.identity)
        .collect()
}

fn kill(pid: u32) -> bool {
    if pid == std::process::id() {
        return false;
    }
    let Ok(pid) = i32::try_from(pid) else {
        return false;
    };
    // SAFETY: kill takes plain integers. The caller matched the identity of this process.
    pid > 1 && unsafe { libc::kill(pid, libc::SIGKILL) } == 0
}
