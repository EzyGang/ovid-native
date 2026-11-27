mod record;
mod recovery;

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::workspace::{Workspace, WorkspaceError, sha256};

use record::{RECORD_HEADER, encode, read_records};

/// One file effect of a mutation, as the write-ahead record stores it.
///
/// `before` and `after` are digests of the normalized text. `None` means that the file is
/// absent. `blob` names the content-addressed pre-image bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct JournalEntry {
    pub operation: String,
    pub path: String,
    pub destination: Option<String>,
    pub before: Option<String>,
    pub after: Option<String>,
    pub blob: Option<String>,
    pub staged: bool,
}

/// A published mutation record. A record that `finish_mutation` does not delete stays for
/// recovery at the next open.
#[derive(Debug)]
pub(crate) struct MutationJournal {
    id: String,
    state_dir: PathBuf,
    record_path: PathBuf,
    record: File,
    blobs: Vec<String>,
}

impl JournalEntry {
    pub(crate) fn new(
        operation: &str,
        path: &str,
        destination: Option<String>,
        before: Option<String>,
        after: Option<String>,
    ) -> Self {
        Self {
            operation: operation.to_owned(),
            path: path.to_owned(),
            destination,
            before,
            after,
            blob: None,
            staged: false,
        }
    }
}

impl Workspace {
    /// Stores every pre-image and publishes the record before the first file commit.
    ///
    /// Each new blob and the record are synced once. Without a state directory this does nothing.
    pub(crate) fn begin_mutation<'a, I>(
        &self,
        entries: I,
    ) -> Result<Option<MutationJournal>, WorkspaceError>
    where
        I: IntoIterator<Item = (JournalEntry, Option<&'a Path>)>,
    {
        let Some(state_dir) = self.state.state_dir.as_deref() else {
            return Ok(None);
        };
        let blob_dir = state_dir.join("blobs");
        let mutations = state_dir.join("mutations");
        for directory in [&blob_dir, &mutations] {
            fs::create_dir_all(directory).map_err(|error| {
                WorkspaceError::Write(format!("cannot create the mutation journal: {error}"))
            })?;
        }
        let mut record = format!("{RECORD_HEADER}\n");
        let mut blobs = Vec::new();
        for (mut entry, source) in entries {
            if let Some(source) = source {
                let blob = store_pre_image(&blob_dir, source, &entry.path)?;
                blobs.push(blob.clone());
                entry.blob = Some(blob);
            }
            record.push_str(&encode(&entry));
        }
        let id = mutation_id();
        let record_path = mutations.join(format!("{id}.record"));
        let record = write_synced(&mutations, &record_path, record.as_bytes())?;
        Ok(Some(MutationJournal {
            id,
            state_dir: state_dir.to_path_buf(),
            record_path,
            record,
            blobs,
        }))
    }
}

impl MutationJournal {
    pub(crate) fn staging_name(&self, index: usize) -> String {
        staging_directory(&self.id, index)
    }
}

/// Appends a progress line for a landed file without a sync.
pub(crate) fn mark_landed(journal: Option<&MutationJournal>, index: usize) {
    let Some(journal) = journal else {
        return;
    };
    // Recovery decides by digests, so a lost progress line cannot change its outcome.
    let _ = (&journal.record).write_all(format!("landed\t{index}\n").as_bytes());
    #[cfg(test)]
    if crate::workspace::journal_tests::crash_requested(
        crate::workspace::journal_tests::CrashPoint::Landed(index),
    ) {
        panic!("simulated process loss after file {index}");
    }
}

/// Deletes the record and its unshared blobs, then returns the mutation result.
pub(crate) fn finish_mutation<T>(
    journal: Option<MutationJournal>,
    result: Result<T, WorkspaceError>,
) -> Result<T, WorkspaceError> {
    let finished = journal.map_or(Ok(()), |journal| {
        let MutationJournal {
            state_dir,
            record_path,
            record,
            blobs,
            ..
        } = journal;
        drop(record);
        discard_record(&state_dir, &record_path, blobs.iter().map(String::as_str))
    });
    let value = result?;
    finished.map(|()| value)
}

fn staging_directory(id: &str, index: usize) -> String {
    format!(".ovid-hashline-{id}-{index}")
}

fn store_pre_image(blob_dir: &Path, source: &Path, path: &str) -> Result<String, WorkspaceError> {
    let bytes = fs::read(source).map_err(|error| {
        WorkspaceError::Stale(format!("cannot read pre-image of {path}: {error}"))
    })?;
    let blob = sha256(&bytes);
    let target = blob_dir.join(&blob);
    if !target.exists() {
        write_synced(blob_dir, &target, &bytes)?;
    }
    Ok(blob)
}

fn write_synced(directory: &Path, target: &Path, bytes: &[u8]) -> Result<File, WorkspaceError> {
    let journal_error = |error: std::io::Error| {
        WorkspaceError::Write(format!("cannot write the mutation journal: {error}"))
    };
    let mut temporary = tempfile::NamedTempFile::new_in(directory).map_err(journal_error)?;
    temporary
        .write_all(bytes)
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(journal_error)?;
    temporary
        .persist(target)
        .map_err(|error| journal_error(error.error))
}

fn mutation_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    format!("{nanos:032x}-{:x}", std::process::id())
}

/// Deletes a record, then deletes each of its blobs that no remaining record names.
fn discard_record<'a, I>(state_dir: &Path, record: &Path, blobs: I) -> Result<(), WorkspaceError>
where
    I: IntoIterator<Item = &'a str>,
{
    let write_error = |error: std::io::Error| {
        WorkspaceError::Write(format!("cannot finish the mutation journal: {error}"))
    };
    fs::remove_file(record).map_err(write_error)?;
    let shared = read_records(state_dir)?
        .into_iter()
        .flat_map(|record| record.entries.into_iter().filter_map(|entry| entry.blob))
        .collect::<HashSet<_>>();
    for blob in blobs.into_iter().filter(|blob| !shared.contains(*blob)) {
        if let Err(error) = fs::remove_file(state_dir.join("blobs").join(blob))
            && error.kind() != ErrorKind::NotFound
        {
            return Err(write_error(error));
        }
    }
    Ok(())
}
