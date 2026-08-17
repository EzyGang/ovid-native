use std::fs;

use crate::workspace::content::NormalizedText;
use crate::workspace::path::validate_relative;
use crate::workspace::workflows::{edit_result, load_current};
use crate::workspace::{
    EditResult, FileChange, MutationContext, Workspace, WorkspaceError, atomic_replace_path,
    create_file, move_file_noclobber, sha256,
};

impl Workspace {
    pub(crate) fn create_text_file(
        &self,
        path: &str,
        content: &str,
        create_parents: bool,
        context: &MutationContext,
    ) -> Result<EditResult, WorkspaceError> {
        context.ensure_active()?;

        validate_relative(path)?;
        let _coordinator = self.write_guard()?;
        let text = NormalizedText::from_replacement(content);
        let parents_allowed = create_parents && context.policy.create_parent_directories;
        if create_parents && !parents_allowed {
            return Err(WorkspaceError::Write(format!(
                "workspace policy does not allow parent creation: {path}"
            )));
        }
        context.ensure_active()?;
        create_file(self.root(), path, &text.serialize(), parents_allowed)?;
        let (file_generation, revision) = self.mark_file_changed(path)?;
        let after = sha256(text.source.as_bytes());
        let post = self.record_post_edit(path, &text, file_generation, None, &context.policy)?;
        let observation = post.as_ref().map(|source| source.observation.clone());

        Ok(edit_result(
            context,
            vec![FileChange {
                path: path.to_owned(),
                operation: "create".to_owned(),
                destination: None,
                before_sha256: None,
                after_sha256: Some(after),
                observation,
                file_generation,
                revision,
            }],
            post.into_iter().collect(),
            None,
            None,
        ))
    }

    pub(crate) fn replace_text_file(
        &self,
        path: &str,
        content: &str,
        expected_observation: &str,
        context: &MutationContext,
    ) -> Result<EditResult, WorkspaceError> {
        let _coordinator = self.write_guard()?;
        context.ensure_active()?;

        let (target, current) = load_current(self, path, &context.policy)?;
        let before = sha256(current.source.as_bytes());
        let authorization = self
            .observations()?
            .resolve(path, expected_observation, &before)?;
        authorization.require_complete(path)?;
        let replacement = NormalizedText::from_replacement(content);
        context.ensure_active()?;
        atomic_replace_path(&target, path, &before, &replacement.serialize())?;
        let (file_generation, revision) = self.mark_file_changed(path)?;
        let after = sha256(replacement.source.as_bytes());
        let post =
            self.record_post_edit(path, &replacement, file_generation, None, &context.policy)?;
        let observation = post.as_ref().map(|source| source.observation.clone());

        Ok(edit_result(
            context,
            vec![FileChange {
                path: path.to_owned(),
                operation: "update".to_owned(),
                destination: None,
                before_sha256: Some(before),
                after_sha256: Some(after),
                observation,
                file_generation,
                revision,
            }],
            post.into_iter().collect(),
            None,
            None,
        ))
    }

    pub(crate) fn delete_text_file(
        &self,
        path: &str,
        context: &MutationContext,
    ) -> Result<EditResult, WorkspaceError> {
        let _coordinator = self.write_guard()?;
        context.ensure_active()?;

        let (target, current) = load_current(self, path, &context.policy)?;
        let before = sha256(current.source.as_bytes());
        self.observations()?
            .current(path, &before)?
            .require_complete(path)?;
        context.ensure_active()?;
        fs::remove_file(&target)
            .map_err(|error| WorkspaceError::Write(format!("cannot delete {path}: {error}")))?;
        let (file_generation, revision) = self.mark_file_changed(path)?;

        Ok(edit_result(
            context,
            vec![FileChange {
                path: path.to_owned(),
                operation: "delete".to_owned(),
                destination: None,
                before_sha256: Some(before),
                after_sha256: None,
                observation: None,
                file_generation,
                revision,
            }],
            Vec::new(),
            None,
            None,
        ))
    }

    pub(crate) fn move_text_file(
        &self,
        path: &str,
        destination: &str,
        context: &MutationContext,
    ) -> Result<EditResult, WorkspaceError> {
        let _coordinator = self.write_guard()?;
        let (target, current) = load_current(self, path, &context.policy)?;
        let before = sha256(current.source.as_bytes());
        self.observations()?
            .current(path, &before)?
            .require_complete(path)?;
        context.ensure_active()?;
        let destination_path =
            crate::workspace::path::resolve_new_file(self.root(), destination, false)?;
        context.ensure_active()?;
        move_file_noclobber(&target, &destination_path, path, destination)?;
        let (file_generation, revision) = self.mark_file_changed(destination)?;
        let post = self.record_post_edit(
            destination,
            &current,
            file_generation,
            None,
            &context.policy,
        )?;
        let observation = post.as_ref().map(|source| source.observation.clone());

        Ok(edit_result(
            context,
            vec![FileChange {
                path: path.to_owned(),
                operation: "move".to_owned(),
                destination: Some(destination.to_owned()),
                before_sha256: Some(before.clone()),
                after_sha256: Some(before),
                observation,
                file_generation,
                revision,
            }],
            post.into_iter().collect(),
            None,
            None,
        ))
    }
}
