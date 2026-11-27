use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use crate::workspace::WorkspaceError;
use crate::workspace::journal::JournalEntry;

pub(super) struct StoredRecord {
    pub id: String,
    pub path: PathBuf,
    pub entries: Vec<JournalEntry>,
}

pub(super) const RECORD_HEADER: &str = "ovid-mutation-record 1";

pub(super) fn encode(entry: &JournalEntry) -> String {
    let fields = [
        Some(entry.operation.as_str()),
        Some(entry.path.as_str()),
        entry.destination.as_deref(),
        entry.before.as_deref(),
        entry.after.as_deref(),
        entry.blob.as_deref(),
        entry.staged.then_some("staged"),
    ];
    let mut line = fields.map(|field| escape(field.unwrap_or(""))).join("\t");
    line.push('\n');
    line
}

/// Reads every published record. Progress lines and torn trailing lines are skipped.
pub(super) fn read_records(state_dir: &Path) -> Result<Vec<StoredRecord>, WorkspaceError> {
    let read_error = |error: std::io::Error| {
        WorkspaceError::Read(format!("cannot read the mutation journal: {error}"))
    };
    let listing = match fs::read_dir(state_dir.join("mutations")) {
        Ok(listing) => listing,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(read_error(error)),
    };
    let mut records = Vec::new();
    for item in listing {
        let path = item.map_err(read_error)?.path();
        let Some(id) = path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_suffix(".record"))
            .map(str::to_owned)
        else {
            continue;
        };
        let text = fs::read_to_string(&path).map_err(read_error)?;
        let mut lines = text.lines();
        if lines.next() != Some(RECORD_HEADER) {
            return Err(WorkspaceError::Read(format!(
                "unknown mutation record format: {id}"
            )));
        }
        let entries = lines.filter_map(decode).collect();
        records.push(StoredRecord { id, path, entries });
    }
    records.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(records)
}

fn decode(line: &str) -> Option<JournalEntry> {
    let fields = line.split('\t').map(unescape).collect::<Vec<_>>();
    let [operation, path, destination, before, after, blob, staged] =
        <[String; 7]>::try_from(fields).ok()?;
    let optional = |value: String| (!value.is_empty()).then_some(value);
    Some(JournalEntry {
        operation,
        path,
        destination: optional(destination),
        before: optional(before),
        after: optional(after),
        blob: optional(blob),
        staged: staged == "staged",
    })
}

fn escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

fn unescape(value: &str) -> String {
    let mut decoded = String::with_capacity(value.len());
    let mut characters = value.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            decoded.push(character);
            continue;
        }
        match characters.next() {
            Some('t') => decoded.push('\t'),
            Some('n') => decoded.push('\n'),
            Some('r') => decoded.push('\r'),
            Some(other) => decoded.push(other),
            None => (),
        }
    }
    decoded
}
