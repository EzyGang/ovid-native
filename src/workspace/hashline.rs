use std::fs;
use std::path::{Path, PathBuf};

use crate::workspace::content::NormalizedText;
use crate::workspace::hashline_plan::build_plan;
use crate::workspace::hashline_types::{HashlineFilePlan, HashlineSection};
use crate::workspace::path::resolve_new_file;
use crate::workspace::workflows::edit_result;
use crate::workspace::{
    EditResult, FileChange, MutationContext, Workspace, WorkspaceError, atomic_replace_path,
    ensure_current_file, sha256,
};

impl Workspace {
    pub(crate) fn apply_hashline(
        &self,
        sections: &[HashlineSection],
        context: &MutationContext,
    ) -> Result<EditResult, WorkspaceError> {
        self.validate_mutation(context, "hashline")?;
        let _coordinator = self.write_guard()?;
        context.ensure_active()?;
        let mut plan = build_plan(self, sections, context)?;
        let pending = plan
            .files
            .iter()
            .map(|file| file.path.clone())
            .collect::<Vec<_>>();
        let mut changes = Vec::new();
        let mut posts = Vec::new();

        for (index, file) in plan.files.iter_mut().enumerate() {
            context.ensure_active()?;
            let result = self.commit_hashline_file(file, context);
            match result {
                Ok((change, post)) => {
                    changes.push(change);
                    posts.extend(post);
                }
                Err(error) if changes.is_empty() => return Err(error),
                Err(_) => {
                    return Err(WorkspaceError::PartialCommit {
                        landed: pending[..index].to_vec(),
                        pending: pending[index..].to_vec(),
                        changes,
                    });
                }
            }
        }
        self.replace_named_registers(plan.named_registers)?;

        Ok(edit_result(context, changes, posts, None, None))
    }

    fn commit_hashline_file(
        &self,
        file: &mut HashlineFilePlan,
        context: &MutationContext,
    ) -> Result<(FileChange, Option<crate::workspace::PostEditSource>), WorkspaceError> {
        let before = sha256(file.current.source.as_bytes());
        ensure_current_file(&file.target, &file.path, &before, &file.identity)?;
        if file.remove {
            stage_hashline_file(file, &before)?.remove()?;
            let (generation, revision) = self.mark_file_changed(&file.path)?;
            return Ok((
                file_change(file, "delete", None, &before, None, generation, revision),
                None,
            ));
        }

        let changed = file.final_source != file.current.source;
        let final_path = match file.destination.as_deref() {
            Some(destination) => {
                let destination_path = resolve_new_file(self.root(), destination, false)?;
                let staged = stage_hashline_file(file, &before)?;
                staged.move_to(
                    &destination_path,
                    destination,
                    changed.then(|| file.current.serialize_with_current(&file.final_source)),
                )?;
                destination
            }
            None => {
                if changed {
                    let bytes = file.current.serialize_with_current(&file.final_source);
                    atomic_replace_path(&file.target, &file.path, &before, &bytes)?;
                }
                &file.path
            }
        };
        let (generation, revision) = self.mark_file_changed(final_path)?;
        let final_text =
            NormalizedText::decode(file.current.serialize_with_current(&file.final_source))?;
        let after = sha256(final_text.source.as_bytes());
        let post = self.record_post_edit(
            final_path,
            &final_text,
            generation,
            file.changed_range,
            &context.policy,
        )?;
        let observation = post.as_ref().map(|source| source.observation.clone());
        let operation = if file.destination.is_some() {
            "move"
        } else {
            "update"
        };
        let change = file_change(
            file,
            operation,
            observation,
            &before,
            Some(after),
            generation,
            revision,
        );
        Ok((change, post))
    }
}

struct StagedHashlineFile {
    directory: tempfile::TempDir,
    source: PathBuf,
    original: PathBuf,
    relative: String,
}

impl StagedHashlineFile {
    fn remove(self) -> Result<(), WorkspaceError> {
        match fs::remove_file(&self.source) {
            Ok(()) => Ok(()),
            Err(error) => {
                let message =
                    WorkspaceError::Write(format!("cannot delete {}: {error}", self.relative));
                Err(self.restore_or(message))
            }
        }
    }

    fn move_to(
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

fn stage_hashline_file(
    file: &HashlineFilePlan,
    expected_sha256: &str,
) -> Result<StagedHashlineFile, WorkspaceError> {
    let parent = file
        .target
        .parent()
        .ok_or_else(|| WorkspaceError::Write(format!("path has no parent: {}", file.path)))?;
    let directory = tempfile::Builder::new()
        .prefix(".ovid-hashline-")
        .tempdir_in(parent)
        .map_err(|error| {
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
    let staged = StagedHashlineFile {
        directory,
        source,
        original: file.target.clone(),
        relative: file.path.clone(),
    };
    if let Err(error) =
        ensure_current_file(&staged.source, &file.path, expected_sha256, &file.identity)
    {
        return Err(staged.restore_or(error));
    }
    Ok(staged)
}

fn file_change(
    file: &HashlineFilePlan,
    operation: &str,
    observation: Option<crate::workspace::ObservationReceipt>,
    before: &str,
    after: Option<String>,
    file_generation: u64,
    revision: u64,
) -> FileChange {
    FileChange {
        path: file.path.clone(),
        operation: operation.to_owned(),
        destination: file.destination.clone(),
        before_sha256: Some(before.to_owned()),
        after_sha256: after,
        observation,
        file_generation,
        revision,
    }
}
