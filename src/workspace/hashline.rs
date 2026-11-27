use std::path::Path;

use crate::workspace::content::NormalizedText;
use crate::workspace::hashline_plan::build_plan;
use crate::workspace::hashline_stage::stage_hashline_file;
use crate::workspace::hashline_types::{HashlineFilePlan, HashlineSection};
use crate::workspace::journal::{JournalEntry, MutationJournal, finish_mutation, mark_landed};
use crate::workspace::path::resolve_new_file;
use crate::workspace::workflows::edit_result;
use crate::workspace::{
    EditResult, FileChange, MutationContext, PostEditSource, Workspace, WorkspaceError,
    atomic_replace_path, ensure_current_file, sha256,
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
        let journal = self.begin_mutation(plan.files.iter().map(journal_entry))?;
        let committed = self.commit_hashline_files(&mut plan.files, context, journal.as_ref());
        let (changes, posts) = finish_mutation(journal, committed)?;
        self.replace_named_registers(plan.named_registers)?;

        Ok(edit_result(context, changes, posts, None, None))
    }

    fn commit_hashline_files(
        &self,
        files: &mut [HashlineFilePlan],
        context: &MutationContext,
        journal: Option<&MutationJournal>,
    ) -> Result<(Vec<FileChange>, Vec<PostEditSource>), WorkspaceError> {
        let pending = files
            .iter()
            .map(|file| file.path.clone())
            .collect::<Vec<_>>();
        let mut changes = Vec::new();
        let mut posts = Vec::new();

        for (index, file) in files.iter_mut().enumerate() {
            context.ensure_active()?;
            let staging = journal.map(|journal| journal.staging_name(index));
            match self.commit_hashline_file(file, context, staging.as_deref()) {
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
            mark_landed(journal, index);
        }

        Ok((changes, posts))
    }

    fn commit_hashline_file(
        &self,
        file: &mut HashlineFilePlan,
        context: &MutationContext,
        staging: Option<&str>,
    ) -> Result<(FileChange, Option<PostEditSource>), WorkspaceError> {
        let before = sha256(file.current.source.as_bytes());
        let identity = file.identity.take().ok_or_else(|| {
            WorkspaceError::Stale(format!(
                "Hashline source identity is unavailable: {}",
                file.path
            ))
        })?;
        ensure_current_file(&file.target, &file.path, &before, &identity)?;
        if file.remove {
            let staged = stage_hashline_file(file, &before, &identity, staging)?;
            drop(identity);
            staged.remove()?;
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
                let staged = stage_hashline_file(file, &before, &identity, staging)?;
                drop(identity);
                staged.move_to(
                    &destination_path,
                    destination,
                    changed.then(|| file.current.serialize_with_current(&file.final_source)),
                )?;
                destination
            }
            None => {
                drop(identity);
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

fn journal_entry(file: &HashlineFilePlan) -> (JournalEntry, Option<&Path>) {
    let staged = file.remove || file.destination.is_some();
    let touched = staged || file.final_source != file.current.source;
    let operation = match (file.remove, file.destination.is_some()) {
        (true, _) => "delete",
        (false, true) => "move",
        (false, false) => "update",
    };
    let entry = JournalEntry {
        staged,
        ..JournalEntry::new(
            operation,
            &file.path,
            file.destination.clone(),
            Some(sha256(file.current.source.as_bytes())),
            (!file.remove).then(|| sha256(file.final_source.as_bytes())),
        )
    };
    (entry, touched.then_some(file.target.as_path()))
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
