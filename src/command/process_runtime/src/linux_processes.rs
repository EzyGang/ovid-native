use std::collections::HashMap;
use std::fs;

use crate::ProcessIdentity;
use crate::unix_orphans::Inspected;

pub(crate) fn inspect(pid: u32) -> Option<Inspected> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // The command name can hold spaces and parentheses, so the fields start after the last one.
    let fields = stat
        .get(stat.rfind(')')? + 1..)?
        .split_whitespace()
        .collect::<Vec<_>>();
    if matches!(fields.first(), None | Some(&("Z" | "X"))) {
        return None;
    }
    Some(Inspected {
        identity: ProcessIdentity {
            pid,
            start_time: fields.get(19)?.parse().ok()?,
        },
        group: fields.get(2)?.parse().ok()?,
    })
}

pub(crate) fn group_members(groups: &[i32]) -> HashMap<i32, Vec<ProcessIdentity>> {
    let mut members = HashMap::<i32, Vec<ProcessIdentity>>::new();
    if groups.is_empty() {
        return members;
    }
    let Ok(listing) = fs::read_dir("/proc") else {
        return members;
    };
    let pids = listing
        .flatten()
        .filter_map(|entry| entry.file_name().to_str()?.parse::<u32>().ok());
    for process in pids.filter_map(inspect) {
        if groups.contains(&process.group) {
            members
                .entry(process.group)
                .or_default()
                .push(process.identity);
        }
    }
    members
}

pub(crate) fn boot_id() -> Option<String> {
    fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .ok()
        .map(|id| id.trim().to_owned())
}
