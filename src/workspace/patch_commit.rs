use std::fs;

use crate::workspace::patch_plan::PreparedOperation;
use crate::workspace::workflows::edit_result;
use crate::workspace::{
    EditResult, FileChange, MutationContext, PostEditSource, Workspace, WorkspaceError,
    atomic_replace_path, create_file, move_file_noclobber, sha256,
};

impl Workspace {
    pub(super) fn commit_patch(
        &self,
        prepared: Vec<PreparedOperation>,
        context: &MutationContext,
    ) -> Result<EditResult, WorkspaceError> {
        let pending = prepared
            .iter()
            .map(PreparedOperation::summary)
            .collect::<Vec<_>>();
        let mut landed = Vec::new();
        let mut changes = Vec::with_capacity(prepared.len());
        let mut post_edit_sources = Vec::new();
        for (index, operation) in prepared.into_iter().enumerate() {
            let result = self.commit_patch_operation(
                operation,
                context,
                &mut changes,
                &mut post_edit_sources,
            );
            match result {
                Ok(summary) => landed.push(summary),
                Err(WorkspaceError::PartialCommit {
                    landed: operation_landed,
                    pending: operation_pending,
                    ..
                }) => {
                    landed.extend(operation_landed);
                    let mut remaining = operation_pending;
                    remaining.extend_from_slice(&pending[index + 1..]);
                    return Err(WorkspaceError::PartialCommit {
                        landed,
                        pending: remaining,
                        changes,
                    });
                }
                Err(error) if landed.is_empty() => return Err(error),
                Err(_) => {
                    return Err(WorkspaceError::PartialCommit {
                        landed,
                        pending: pending[index..].to_vec(),
                        changes,
                    });
                }
            }
        }

        Ok(edit_result(context, changes, post_edit_sources, None, None))
    }

    fn commit_patch_operation(
        &self,
        operation: PreparedOperation,
        context: &MutationContext,
        changes: &mut Vec<FileChange>,
        post_edit_sources: &mut Vec<PostEditSource>,
    ) -> Result<String, WorkspaceError> {
        context.ensure_active()?;

        match operation {
            PreparedOperation::Create { path, text } => {
                let create_parents = context.policy.create_parent_directories;
                context.ensure_active()?;
                create_file(self.root(), &path, &text.serialize(), create_parents)?;
                let (file_generation, revision) = self.mark_file_changed(&path)?;
                let after = sha256(text.source.as_bytes());
                let post =
                    self.record_post_edit(&path, &text, file_generation, None, &context.policy)?;
                let observation = post.as_ref().map(|source| source.observation.clone());
                post_edit_sources.extend(post);
                changes.push(FileChange {
                    path: path.clone(),
                    operation: "create".to_owned(),
                    destination: None,
                    before_sha256: None,
                    after_sha256: Some(after),
                    observation,
                    file_generation,
                    revision,
                });
                Ok(path)
            }
            PreparedOperation::Update {
                path,
                target,
                destination,
                before,
                text,
                bytes,
                changed,
            } => {
                context.ensure_active()?;
                atomic_replace_path(&target, &path, &before, &bytes)?;
                let (result_path, operation, destination_name) = match destination {
                    Some((name, destination_target)) => {
                        context.ensure_active()?;
                        move_file_noclobber(&target, &destination_target, &path, &name)?;
                        (name.clone(), "move", Some(name))
                    }
                    None => (path.clone(), "update", None),
                };
                let (file_generation, revision) = self.mark_file_changed(&result_path)?;
                let after = sha256(text.source.as_bytes());
                let post = self.record_post_edit(
                    &result_path,
                    &text,
                    file_generation,
                    changed,
                    &context.policy,
                )?;
                let observation = post.as_ref().map(|source| source.observation.clone());
                post_edit_sources.extend(post);
                changes.push(FileChange {
                    path: path.clone(),
                    operation: operation.to_owned(),
                    destination: destination_name,
                    before_sha256: Some(before),
                    after_sha256: Some(after),
                    observation,
                    file_generation,
                    revision,
                });
                Ok(path)
            }
            PreparedOperation::Delete {
                path,
                target,
                before,
            } => {
                context.ensure_active()?;
                fs::remove_file(&target).map_err(|error| {
                    WorkspaceError::Write(format!("cannot delete {path}: {error}"))
                })?;
                let (file_generation, revision) = self.mark_file_changed(&path)?;
                changes.push(FileChange {
                    path: path.clone(),
                    operation: "delete".to_owned(),
                    destination: None,
                    before_sha256: Some(before),
                    after_sha256: None,
                    observation: None,
                    file_generation,
                    revision,
                });
                Ok(path)
            }
        }
    }
}
