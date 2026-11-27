use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use crate::workspace::journal::record::{StoredRecord, read_records};
use crate::workspace::journal::{JournalEntry, discard_record, staging_directory};
use crate::workspace::{
    NormalizedText, Workspace, WorkspaceError, atomic_replace_path, create_file, sha256,
};

/// The outcome of one leftover mutation record. It holds paths only, never file contents.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct RecoveryReport {
    pub mutation_id: String,
    pub restored: Vec<String>,
    pub conflicts: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Outcome {
    Restored,
    Pending,
    Conflict,
}

#[derive(Debug, Eq, PartialEq)]
enum FileState {
    Missing,
    Text(String),
    Unreadable,
}

impl Workspace {
    /// Rolls back every leftover mutation record under the exclusive write guard.
    ///
    /// A file returns to its pre-image only when it holds the pre-image or the post-image.
    /// Other content is a conflict and stays untouched. Every record is deleted.
    pub(crate) fn recover(&self) -> Result<Vec<RecoveryReport>, WorkspaceError> {
        let _guard = self.write_guard()?;
        let Some(state_dir) = self.state.state_dir.as_deref() else {
            return Ok(Vec::new());
        };
        read_records(state_dir)?
            .into_iter()
            .map(|record| self.recover_record(state_dir, record))
            .collect()
    }

    fn recover_record(
        &self,
        state_dir: &Path,
        record: StoredRecord,
    ) -> Result<RecoveryReport, WorkspaceError> {
        let blob_dir = state_dir.join("blobs");
        let mut report = RecoveryReport {
            mutation_id: record.id.clone(),
            ..RecoveryReport::default()
        };
        for (index, entry) in record.entries.iter().enumerate() {
            let target = self.root().join(&entry.path);
            let staging = target
                .parent()
                .filter(|_| entry.staged)
                .map(|parent| parent.join(staging_directory(&record.id, index)));
            match self.recover_entry(&blob_dir, entry, &target, staging.as_deref())? {
                Outcome::Restored => report.restored.push(entry.path.clone()),
                Outcome::Conflict => report.conflicts.push(entry.path.clone()),
                Outcome::Pending => (),
            }
            if let Some(staging) = staging
                && let Err(error) = fs::remove_dir_all(&staging)
                && error.kind() != ErrorKind::NotFound
            {
                return Err(write_error(&entry.path, &error));
            }
        }
        let blobs = record
            .entries
            .iter()
            .filter_map(|entry| entry.blob.as_deref());
        discard_record(state_dir, &record.path, blobs)?;
        Ok(report)
    }

    fn recover_entry(
        &self,
        blob_dir: &Path,
        entry: &JournalEntry,
        target: &Path,
        staging: Option<&Path>,
    ) -> Result<Outcome, WorkspaceError> {
        let unstaged = match staging {
            Some(staging) => restore_staged(&staging.join("source"), target, entry)?,
            None => false,
        };
        let current = file_state(target);
        let landed = entry
            .destination
            .as_deref()
            .map(|destination| {
                let path = self.root().join(destination);
                let state = file_state(&path);
                (path, state)
            })
            .filter(|(_, state)| *state != FileState::Missing);
        let outcome = match landed {
            Some((destination, state)) => {
                self.recover_move(blob_dir, entry, &current, &destination, &state)?
            }
            None if current.matches(entry.before.as_deref()) => Outcome::Pending,
            None if current.matches(entry.after.as_deref()) => {
                self.restore(blob_dir, entry, &current)?
            }
            None => Outcome::Conflict,
        };
        Ok(match outcome {
            Outcome::Pending if unstaged => Outcome::Restored,
            outcome => outcome,
        })
    }

    fn recover_move(
        &self,
        blob_dir: &Path,
        entry: &JournalEntry,
        current: &FileState,
        destination: &Path,
        landed: &FileState,
    ) -> Result<Outcome, WorkspaceError> {
        if !landed.matches(entry.after.as_deref()) {
            return Ok(Outcome::Conflict);
        }
        let outcome = if current.matches(entry.before.as_deref()) {
            Outcome::Restored
        } else if *current == FileState::Missing || current.matches(entry.after.as_deref()) {
            self.restore(blob_dir, entry, current)?
        } else {
            Outcome::Conflict
        };
        if outcome == Outcome::Restored {
            fs::remove_file(destination).map_err(|error| write_error(&entry.path, &error))?;
        }
        Ok(outcome)
    }

    fn restore(
        &self,
        blob_dir: &Path,
        entry: &JournalEntry,
        current: &FileState,
    ) -> Result<Outcome, WorkspaceError> {
        let target = self.root().join(&entry.path);
        let Some(blob) = entry.blob.as_deref() else {
            fs::remove_file(&target).map_err(|error| write_error(&entry.path, &error))?;
            return Ok(Outcome::Restored);
        };
        let Some(bytes) = fs::read(blob_dir.join(blob))
            .ok()
            .filter(|bytes| sha256(bytes) == blob)
        else {
            return Ok(Outcome::Conflict);
        };
        match current {
            FileState::Text(digest) => atomic_replace_path(&target, &entry.path, digest, &bytes)?,
            FileState::Missing | FileState::Unreadable => {
                create_file(self.root(), &entry.path, &bytes, true).map(drop)?;
            }
        }
        Ok(Outcome::Restored)
    }
}

impl FileState {
    fn matches(&self, expected: Option<&str>) -> bool {
        match (self, expected) {
            (Self::Missing, None) => true,
            (Self::Text(digest), Some(expected)) => digest == expected,
            _ => false,
        }
    }
}

fn restore_staged(
    source: &Path,
    target: &Path,
    entry: &JournalEntry,
) -> Result<bool, WorkspaceError> {
    if file_state(target) != FileState::Missing
        || !file_state(source).matches(entry.before.as_deref())
    {
        return Ok(false);
    }
    fs::rename(source, target).map_err(|error| write_error(&entry.path, &error))?;
    Ok(true)
}

fn file_state(path: &Path) -> FileState {
    match fs::read(path) {
        Ok(bytes) => NormalizedText::decode(bytes).map_or(FileState::Unreadable, |text| {
            FileState::Text(sha256(text.source.as_bytes()))
        }),
        Err(error) if error.kind() == ErrorKind::NotFound => FileState::Missing,
        Err(_) => FileState::Unreadable,
    }
}

fn write_error(path: &str, error: &std::io::Error) -> WorkspaceError {
    WorkspaceError::Write(format!("cannot recover {path}: {error}"))
}
