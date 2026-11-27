use std::fs;
use std::path::{Path, PathBuf};

use crate::workspace::content::NormalizedText;
use crate::workspace::hashline_types::HashlineFilePlan;
use crate::workspace::{
    FileIdentity, WorkspaceError, atomic_replace_path, ensure_current_file, sha256,
};

pub(crate) struct StagedHashlineFile {
    directory: tempfile::TempDir,
    source: PathBuf,
    original: PathBuf,
    relative: String,
}

impl StagedHashlineFile {
    pub(crate) fn remove(self) -> Result<(), WorkspaceError> {
        match fs::remove_file(&self.source) {
            Ok(()) => Ok(()),
            Err(error) => {
                let message =
                    WorkspaceError::Write(format!("cannot delete {}: {error}", self.relative));
                Err(self.restore_or(message))
            }
        }
    }

    pub(crate) fn move_to(
        self,
        destination: &Path,
        destination_relative: &str,
        replacement: Option<Vec<u8>>,
    ) -> Result<(), WorkspaceError> {
        let final_source = match replacement {
            Some(bytes) => {
                let final_source = self.directory.path().join("final");
                if let Err(error) = fs::copy(&self.source, &final_source) {
                    return Err(self.restore_or(WorkspaceError::Write(format!(
                        "cannot prepare move to {destination_relative}: {error}"
                    ))));
                }
                let before = fs::read(&final_source)
                    .map(|source| {
                        NormalizedText::decode(source).map(|text| sha256(text.source.as_bytes()))
                    })
                    .map_err(|error| {
                        WorkspaceError::Write(format!(
                            "cannot inspect move source for {destination_relative}: {error}"
                        ))
                    })
                    .and_then(|result| result);
                let before = match before {
                    Ok(before) => before,
                    Err(error) => return Err(self.restore_or(error)),
                };
                if let Err(error) =
                    atomic_replace_path(&final_source, destination_relative, &before, &bytes)
                {
                    return Err(self.restore_or(error));
                }
                final_source
            }
            None => self.source.clone(),
        };
        if let Err(error) = fs::hard_link(&final_source, destination) {
            let message = WorkspaceError::Write(format!(
                "cannot move {} to {destination_relative}: {error}",
                self.relative
            ));
            return Err(self.restore_or(message));
        }
        if let Err(error) = fs::remove_file(&self.source) {
            let rollback = fs::remove_file(destination);
            if rollback.is_ok() {
                let message = WorkspaceError::Write(format!(
                    "cannot remove move source {}: {error}",
                    self.relative
                ));
                return Err(self.restore_or(message));
            }
            let pending = self.relative.clone();
            let _ = self.directory.keep();
            return Err(WorkspaceError::PartialCommit {
                landed: vec![destination_relative.to_owned()],
                pending: vec![pending],
                changes: Vec::new(),
            });
        }
        if final_source != self.source && fs::remove_file(&final_source).is_err() {
            let _ = self.directory.keep();
            return Err(WorkspaceError::PartialCommit {
                landed: vec![destination_relative.to_owned()],
                pending: Vec::new(),
                changes: Vec::new(),
            });
        }
        Ok(())
    }

    fn restore_or(self, error: WorkspaceError) -> WorkspaceError {
        if fs::hard_link(&self.source, &self.original).is_ok() {
            return error;
        }
        let pending = self.relative.clone();
        let _ = self.directory.keep();
        WorkspaceError::PartialCommit {
            landed: Vec::new(),
            pending: vec![pending],
            changes: Vec::new(),
        }
    }
}

/// Moves the file into a `.ovid-hashline-*` directory. A journalled mutation passes the
/// directory name that its record names, so recovery can find the staged original.
pub(crate) fn stage_hashline_file(
    file: &HashlineFilePlan,
    expected_sha256: &str,
    expected_identity: &FileIdentity,
    staging: Option<&str>,
) -> Result<StagedHashlineFile, WorkspaceError> {
    let parent = file
        .target
        .parent()
        .ok_or_else(|| WorkspaceError::Write(format!("path has no parent: {}", file.path)))?;
    let mut builder = tempfile::Builder::new();
    builder.prefix(staging.unwrap_or(".ovid-hashline-"));
    if staging.is_some() {
        builder.rand_bytes(0);
    }
    let directory = builder.tempdir_in(parent).map_err(|error| {
        WorkspaceError::Write(format!(
            "cannot stage Hashline source {}: {error}",
            file.path
        ))
    })?;
    let source = directory.path().join("source");
    fs::rename(&file.target, &source).map_err(|error| {
        WorkspaceError::Write(format!(
            "cannot stage Hashline source {}: {error}",
            file.path
        ))
    })?;
    #[cfg(test)]
    if crate::workspace::journal_tests::crash_requested(
        crate::workspace::journal_tests::CrashPoint::Staged,
    ) {
        let _kept = directory.keep();
        panic!("simulated process loss after staging");
    }
    let staged = StagedHashlineFile {
        directory,
        source,
        original: file.target.clone(),
        relative: file.path.clone(),
    };
    if let Err(error) = ensure_current_file(
        &staged.source,
        &file.path,
        expected_sha256,
        expected_identity,
    ) {
        return Err(staged.restore_or(error));
    }
    Ok(staged)
}
