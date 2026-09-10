use std::{collections::HashSet, path::PathBuf};

use crate::workspace::patch::{PatchOperation, PatchOperationKind};
use crate::workspace::path::resolve_new_file;
use crate::workspace::unified_diff::apply_unified_diff;
use crate::workspace::workflows::load_current;
use crate::workspace::{
    LineRange, MutationContext, NormalizedText, Workspace, WorkspaceError, sha256,
};

#[derive(Debug)]
pub(super) enum PreparedOperation {
    Create {
        path: String,
        text: NormalizedText,
    },
    Update {
        path: String,
        target: PathBuf,
        destination: Option<(String, PathBuf)>,
        before: String,
        text: NormalizedText,
        bytes: Vec<u8>,
        changed: Option<LineRange>,
    },
    Delete {
        path: String,
        target: PathBuf,
        before: String,
    },
}

impl PreparedOperation {
    pub(super) fn summary(&self) -> String {
        match self {
            Self::Create { path, .. } | Self::Update { path, .. } | Self::Delete { path, .. } => {
                path.clone()
            }
        }
    }
}

impl Workspace {
    pub(super) fn preflight_patch(
        &self,
        operations: &[PatchOperation],
        context: &MutationContext,
    ) -> Result<Vec<PreparedOperation>, WorkspaceError> {
        let mut prepared = Vec::with_capacity(operations.len());
        let mut paths = HashSet::new();
        for operation in operations {
            if !paths.insert(operation.path.clone()) {
                return Err(WorkspaceError::Patch(format!(
                    "workspace patch contains duplicate source path: {}",
                    operation.path
                )));
            }
            if operation.kind != PatchOperationKind::Update && operation.destination.is_some() {
                return Err(WorkspaceError::Patch(format!(
                    "only update patches accept a destination: {}",
                    operation.path
                )));
            }
            prepared.push(match operation.kind {
                PatchOperationKind::Create => operation.prepare_create(self, context)?,
                PatchOperationKind::Update => {
                    operation.prepare_update(self, context, &mut paths)?
                }
                PatchOperationKind::Delete => operation.prepare_delete(self, context)?,
            });
        }
        Ok(prepared)
    }
}

impl PatchOperation {
    fn prepare_create(
        &self,
        workspace: &Workspace,
        context: &MutationContext,
    ) -> Result<PreparedOperation, WorkspaceError> {
        let body = self.body.as_deref().ok_or_else(|| {
            WorkspaceError::Patch(format!("create patch has no content: {}", self.path))
        })?;
        let text = NormalizedText::from_replacement(body);
        let create_parents = context.policy.create_parent_directories;
        resolve_new_file(workspace.root(), &self.path, create_parents).map(drop)?;
        Ok(PreparedOperation::Create {
            path: self.path.clone(),
            text,
        })
    }

    fn prepare_update(
        &self,
        workspace: &Workspace,
        context: &MutationContext,
        paths: &mut HashSet<String>,
    ) -> Result<PreparedOperation, WorkspaceError> {
        let body = self.body.as_deref().ok_or_else(|| {
            WorkspaceError::Patch(format!("update patch has no diff: {}", self.path))
        })?;
        let (target, current) = load_current(workspace, &self.path, &context.policy)?;
        let before = sha256(current.source.as_bytes());
        let authorization = workspace.observations()?.current(&self.path, &before)?;
        if self.destination.is_some() {
            authorization.require_complete(&self.path)?;
        }
        let (source, required, changed) = apply_unified_diff(
            &current,
            body,
            context.policy.allow_fuzzy_replace,
            context.policy.fuzzy_replace_threshold,
        )?;
        authorization.require_lines(&self.path, &current, &required)?;
        let text = NormalizedText::from_replacement(&source);
        let bytes = current.serialize_with_current(&text.source);
        let destination = self
            .destination
            .as_deref()
            .map(|path| {
                if !paths.insert(path.to_owned()) {
                    return Err(WorkspaceError::Patch(format!(
                        "workspace patch contains a duplicate path: {path}"
                    )));
                }
                resolve_new_file(workspace.root(), path, false)
                    .map(|target| (path.to_owned(), target))
            })
            .transpose()?;
        Ok(PreparedOperation::Update {
            path: self.path.clone(),
            target,
            destination,
            before,
            text,
            bytes,
            changed,
        })
    }

    fn prepare_delete(
        &self,
        workspace: &Workspace,
        context: &MutationContext,
    ) -> Result<PreparedOperation, WorkspaceError> {
        if self.body.is_some() || self.destination.is_some() {
            return Err(WorkspaceError::Patch(format!(
                "delete patch cannot contain content or destination: {}",
                self.path
            )));
        }
        let (target, current) = load_current(workspace, &self.path, &context.policy)?;
        let before = sha256(current.source.as_bytes());
        workspace
            .observations()?
            .current(&self.path, &before)?
            .require_complete(&self.path)?;
        Ok(PreparedOperation::Delete {
            path: self.path.clone(),
            target,
            before,
        })
    }
}
