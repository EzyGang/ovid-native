mod reap;
mod sampler;
#[cfg(test)]
mod tests;

use std::fs;
use std::io::{self, ErrorKind, Write};
use std::path::Path;
use std::sync::LazyLock;

use ovid_command_process::{ProcessIdentity, boot_id};

pub(crate) use reap::reap_orphans;
pub(crate) use sampler::{terminate, track};

/// What owns a recorded process scope.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub(crate) enum ScopeKind {
    Command,
    Job,
}

/// The recorded processes of one live scope. The command is stored as a SHA-256 digest only.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ScopeEntry {
    pub kind: ScopeKind,
    pub owner_tag: Option<String>,
    pub command_hash: String,
    pub started_ms: u64,
    pub group: Option<i32>,
    pub members: Vec<ProcessIdentity>,
}

/// The process record file of one host process.
///
/// Each host writes only its own file, `processes/<pid>-<start time>.record` in the state
/// directory. The boot ID is `None` on platforms whose start times do not depend on the boot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct HostRecord {
    pub host: ProcessIdentity,
    pub boot_id: Option<String>,
    pub entries: Vec<ScopeEntry>,
}

/// This host process, with no entries. It is `None` when the platform cannot identify the host.
static HOST: LazyLock<Option<HostRecord>> = LazyLock::new(|| {
    Some(HostRecord {
        host: ProcessIdentity::of(std::process::id())?,
        boot_id: boot_id(),
        entries: Vec::new(),
    })
});

const RECORD_HEADER: &str = "ovid-process-record 1";

impl HostRecord {
    pub(crate) fn file_name(host: ProcessIdentity) -> String {
        format!("{}-{}.record", host.pid, host.start_time)
    }

    pub(crate) fn encode(&self) -> String {
        let mut text = format!(
            "{RECORD_HEADER}\nhost\t{}\t{}\t{}\n",
            self.host.pid,
            self.host.start_time,
            self.boot_id.as_deref().unwrap_or("")
        );
        for entry in &self.entries {
            let kind = match entry.kind {
                ScopeKind::Command => "command",
                ScopeKind::Job => "job",
            };
            let members = entry
                .members
                .iter()
                .map(|member| format!("{}:{}", member.pid, member.start_time))
                .collect::<Vec<_>>()
                .join(",");
            text.push_str(&format!(
                "scope\t{kind}\t{}\t{}\t{}\t{}\t{members}\n",
                entry.owner_tag.as_deref().map(hex).unwrap_or_default(),
                entry.command_hash,
                entry.started_ms,
                entry
                    .group
                    .map(|group| group.to_string())
                    .unwrap_or_default(),
            ));
        }
        text
    }

    /// Reads an encoded record. Any malformed line rejects the whole record.
    pub(crate) fn decode(text: &str) -> Option<Self> {
        let mut lines = text.lines();
        if lines.next() != Some(RECORD_HEADER) {
            return None;
        }
        let host = lines.next()?.strip_prefix("host\t")?;
        let [pid, start_time, boot] =
            <[&str; 3]>::try_from(host.split('\t').collect::<Vec<_>>()).ok()?;
        Some(Self {
            host: ProcessIdentity {
                pid: pid.parse().ok()?,
                start_time: start_time.parse().ok()?,
            },
            boot_id: (!boot.is_empty()).then(|| boot.to_owned()),
            entries: lines.map(decode_entry).collect::<Option<_>>()?,
        })
    }
}

/// Replaces the record file atomically, or deletes it when the record has no entries.
fn write_record(directory: &Path, record: &HostRecord) -> io::Result<()> {
    let path = directory.join(HostRecord::file_name(record.host));
    if record.entries.is_empty() {
        return remove_record(&path);
    }
    fs::create_dir_all(directory)?;
    let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
    temporary.write_all(record.encode().as_bytes())?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(&path)
        .map(drop)
        .map_err(|error| error.error)
}

fn remove_record(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() != ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

fn decode_entry(line: &str) -> Option<ScopeEntry> {
    let fields = line
        .strip_prefix("scope\t")?
        .split('\t')
        .collect::<Vec<_>>();
    let [kind, owner_tag, command_hash, started_ms, group, members] =
        <[&str; 6]>::try_from(fields).ok()?;
    Some(ScopeEntry {
        kind: match kind {
            "command" => ScopeKind::Command,
            "job" => ScopeKind::Job,
            _ => return None,
        },
        owner_tag: match owner_tag {
            "" => None,
            tag => Some(unhex(tag)?),
        },
        command_hash: command_hash.to_owned(),
        started_ms: started_ms.parse().ok()?,
        group: match group {
            "" => None,
            group => Some(group.parse().ok()?),
        },
        members: members
            .split(',')
            .filter(|member| !member.is_empty())
            .map(|member| {
                let (pid, start_time) = member.split_once(':')?;
                Some(ProcessIdentity {
                    pid: pid.parse().ok()?,
                    start_time: start_time.parse().ok()?,
                })
            })
            .collect::<Option<_>>()?,
    })
}

fn hex(text: &str) -> String {
    text.bytes().map(|byte| format!("{byte:02x}")).collect()
}

fn unhex(text: &str) -> Option<String> {
    let bytes = (0..text.len())
        .step_by(2)
        .map(|start| u8::from_str_radix(text.get(start..start + 2)?, 16).ok())
        .collect::<Option<Vec<_>>>()?;
    String::from_utf8(bytes).ok()
}
