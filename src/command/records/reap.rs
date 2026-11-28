use std::fs;
use std::io::ErrorKind;
use std::time::Duration;

use ovid_command_process::{ProcessIdentity, boot_id, stop_orphans};

use crate::command::CommandError;
use crate::workspace::Workspace;

use super::{HOST, HostRecord, remove_record};

/// One process that a reap stopped. It holds a prefix of the command digest, never the command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ReapedProcess {
    pub pid: u32,
    pub owner_tag: Option<String>,
    pub command_hash_prefix: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ReapReport {
    pub stopped: Vec<ReapedProcess>,
    pub skipped_live_owners: u64,
}

/// Stops the recorded processes of every dead host in the state directory.
///
/// The call skips the file of this host and the file of each host that still runs. It deletes
/// every other file. A record from another boot, a malformed record, and a process whose
/// identity does not match are never signalled.
pub(crate) fn reap_orphans(workspace: &Workspace) -> Result<ReapReport, CommandError> {
    workspace
        .ensure_open()
        .map_err(CommandError::from_workspace)?;
    let mut report = ReapReport::default();
    let Some(state_dir) = workspace.state_dir() else {
        return Ok(report);
    };
    let listing = match fs::read_dir(state_dir.join("processes")) {
        Ok(listing) => listing,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(report),
        Err(error) => return Err(record_error(&error)),
    };
    let own = HOST.as_ref().map(|host| HostRecord::file_name(host.host));
    let boot = boot_id();
    for item in listing {
        let path = item.map_err(|error| record_error(&error))?.path();
        let name = path.file_name().and_then(|name| name.to_str());
        if !name.is_some_and(|name| name.ends_with(".record")) || name == own.as_deref() {
            continue;
        }
        let record = fs::read_to_string(&path)
            .ok()
            .and_then(|text| HostRecord::decode(&text))
            .filter(|record| record.boot_id == boot);
        if let Some(record) = record {
            if ProcessIdentity::of(record.host.pid) == Some(record.host) {
                report.skipped_live_owners += 1;
                continue;
            }
            stop(&record, &mut report);
        }
        remove_record(&path).map_err(|error| record_error(&error))?;
    }
    Ok(report)
}

fn stop(record: &HostRecord, report: &mut ReapReport) {
    for entry in &record.entries {
        let prefix = entry.command_hash.get(..12).unwrap_or(&entry.command_hash);
        let stopped = stop_orphans(entry.group, &entry.members, Duration::from_millis(500));
        report
            .stopped
            .extend(stopped.into_iter().map(|identity| ReapedProcess {
                pid: identity.pid,
                owner_tag: entry.owner_tag.clone(),
                command_hash_prefix: prefix.to_owned(),
            }));
    }
}

fn record_error(error: &std::io::Error) -> CommandError {
    CommandError::Execution(format!("cannot reap the process records: {error}"))
}
