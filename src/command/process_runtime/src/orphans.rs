use std::time::Duration;

use crate::ProcessScope;
#[cfg(unix)]
use crate::unix_orphans as platform;
#[cfg(windows)]
use crate::windows_orphans as platform;

/// Identifies one process across process ID reuse.
///
/// `start_time` is the start time that the platform reports: the creation time on Windows, the
/// clock ticks after boot on Linux, and microseconds after the epoch on macOS. A process that
/// reuses a process ID has a different start time.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct ProcessIdentity {
    pub pid: u32,
    pub start_time: u64,
}

/// The process group and the live processes of one scope at one instant.
///
/// The group is `None` on Windows and for a scope that never started a process. The members are
/// sorted.
#[derive(Clone, Debug, Default, Eq, PartialEq, Hash)]
pub struct ScopeMembers {
    pub group: Option<i32>,
    pub members: Vec<ProcessIdentity>,
}

impl ProcessIdentity {
    /// Returns the identity of the live process `pid`, or `None` when no such process runs.
    #[must_use]
    pub fn of(pid: u32) -> Option<Self> {
        platform::identity(pid)
    }
}

/// Returns the boot ID on Linux, where process start times count from the boot.
///
/// Other platforms return `None`, because their process start times do not depend on the boot.
#[must_use]
pub fn boot_id() -> Option<String> {
    platform::boot_id()
}

/// Lists the live processes of each scope, in the order of `scopes`.
///
/// Windows reads the job of each scope. Linux reads `/proc` once for all scopes. macOS lists the
/// process group of each scope.
#[must_use]
pub fn sample_scopes(scopes: &[&ProcessScope]) -> Vec<ScopeMembers> {
    let mut samples = platform::sample(scopes);
    for sample in &mut samples {
        sample.members.sort_unstable();
    }
    samples
}

/// Stops the recorded processes of a scope whose host is gone, and returns the stopped identities.
///
/// Only a live process whose identity equals a recorded identity is stopped. On Unix, a matching
/// process that is still in `group` proves the group, so the call sends `SIGTERM` to the group,
/// waits at most `grace`, then sends `SIGKILL`. Each other matching process gets `SIGKILL`. On
/// Windows, each matching process is terminated, and `group` and `grace` have no effect.
pub fn stop_orphans(
    group: Option<i32>,
    members: &[ProcessIdentity],
    grace: Duration,
) -> Vec<ProcessIdentity> {
    platform::stop(group, members, grace)
}
