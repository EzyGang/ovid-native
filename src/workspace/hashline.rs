use std::fs;

use crate::workspace::content::NormalizedText;
use crate::workspace::hashline_plan::build_plan;
use crate::workspace::hashline_types::{HashlineFilePlan, HashlineSection};
use crate::workspace::path::resolve_new_file;
use crate::workspace::workflows::edit_result;
use crate::workspace::{
    EditResult, FileChange, MutationContext, Workspace, WorkspaceError, atomic_replace_path,
    move_file_noclobber, sha256,
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
        let plan = build_plan(self, sections, context)?;
        let pending = plan
            .files
            .iter()
            .map(|file| file.path.clone())
            .collect::<Vec<_>>();
        let mut changes = Vec::new();
        let mut posts = Vec::new();

        for (index, file) in plan.files.iter().enumerate() {
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
        file: &HashlineFilePlan,
        context: &MutationContext,
    ) -> Result<(FileChange, Option<crate::workspace::PostEditSource>), WorkspaceError> {
        let before = sha256(file.current.source.as_bytes());
        if file.remove {
            fs::remove_file(&file.target).map_err(|error| {
                WorkspaceError::Write(format!("cannot delete {}: {error}", file.path))
            })?;
            let (generation, revision) = self.mark_file_changed(&file.path)?;
            return Ok((
                file_change(file, "delete", None, &before, None, generation, revision),
                None,
            ));
        }

        let changed = file.final_source != file.current.source;
        if changed {
            let bytes = file.current.serialize_with_current(&file.final_source);
            atomic_replace_path(&file.target, &file.path, &before, &bytes)?;
        }
        let final_path = match file.destination.as_deref() {
            Some(destination) => {
                let destination_path = resolve_new_file(self.root(), destination, false)?;
                move_file_noclobber(&file.target, &destination_path, &file.path, destination)?;
                destination
            }
            None => &file.path,
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
