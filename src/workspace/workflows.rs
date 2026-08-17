use std::fs;

use crate::workspace::content::NormalizedText;
use crate::workspace::path::{normalize_relative, resolve_contained_file};
use crate::workspace::{
    EditResult, FileChange, LineRange, ObservationReceipt, PolicyGeneration, PostEditSource,
    Workspace, WorkspaceError, WorkspacePolicy, sha256,
};

#[derive(Clone, Debug)]
pub(crate) struct MutationContext {
    pub mode: String,
    pub mode_generation: u64,
    pub policy_generation: u64,
    pub policy: WorkspacePolicy,
    pub cancellation: crate::workspace::Cancellation,
}

impl MutationContext {
    pub(crate) fn write(
        policy: PolicyGeneration,
        cancellation: crate::workspace::Cancellation,
    ) -> Self {
        Self {
            mode: "write".to_owned(),
            mode_generation: 1,
            policy_generation: policy.generation,
            policy: policy.policy,
            cancellation,
        }
    }

    pub(crate) fn ensure_active(&self) -> Result<(), WorkspaceError> {
        if self.cancellation.is_cancelled() {
            return Err(WorkspaceError::Cancelled);
        }

        Ok(())
    }
}

impl Workspace {
    pub(crate) fn resolve_observation(
        &self,
        path: &str,
        tag: &str,
    ) -> Result<ObservationReceipt, WorkspaceError> {
        let path = normalize_relative(path)?;
        let policy = self.policy()?;
        let (_, text) = load_current(self, &path, &policy.policy)?;
        let digest = sha256(text.source.as_bytes());
        Ok(self.observations()?.resolve(&path, tag, &digest)?.receipt)
    }

    pub(crate) fn validate_observed_lines(
        &self,
        path: &str,
        tag: &str,
        lines: &[usize],
    ) -> Result<ObservationReceipt, WorkspaceError> {
        let path = normalize_relative(path)?;
        let policy = self.policy()?;
        let (_, text) = load_current(self, &path, &policy.policy)?;
        let digest = sha256(text.source.as_bytes());
        let authorization = self.observations()?.resolve(&path, tag, &digest)?;
        authorization.require_lines(&path, &text, &lines.iter().copied().collect())?;
        Ok(authorization.receipt)
    }

    pub(crate) fn record_post_edit(
        &self,
        path: &str,
        text: &NormalizedText,
        generation: u64,
        changed: Option<LineRange>,
        policy: &WorkspacePolicy,
    ) -> Result<Option<PostEditSource>, WorkspaceError> {
        if text.source.len() as u64 > policy.max_observation_file_bytes {
            return Ok(None);
        }
        let range = changed.or_else(|| {
            (text.total_lines() > 0).then_some(LineRange {
                start: 1,
                end: text.total_lines(),
            })
        });
        let ranges = range.into_iter().collect::<Vec<_>>();
        let lines = crate::workspace::bounded_render(text, &ranges, policy.max_read_bytes);
        let complete_presentation = lines.len() == text.total_lines();
        let observation = self.observations()?.record(
            path,
            text,
            generation,
            &lines,
            complete_presentation,
            (
                policy.max_observation_entries,
                policy.max_observation_store_bytes,
            ),
        )?;

        Ok(Some(PostEditSource {
            path: path.to_owned(),
            observation,
            lines,
            complete_presentation,
        }))
    }
}

pub(crate) fn load_current(
    workspace: &Workspace,
    path: &str,
    policy: &WorkspacePolicy,
) -> Result<(std::path::PathBuf, NormalizedText), WorkspaceError> {
    let target = resolve_contained_file(workspace.root(), path)?;
    let metadata = fs::metadata(&target)
        .map_err(|error| WorkspaceError::Read(format!("cannot inspect {path}: {error}")))?;
    if metadata.len() > policy.max_observation_file_bytes {
        return Err(WorkspaceError::Limit(format!(
            "workspace file exceeds observation limit: {path} ({} > {})",
            metadata.len(),
            policy.max_observation_file_bytes
        )));
    }
    let bytes = fs::read(&target)
        .map_err(|error| WorkspaceError::Read(format!("cannot read {path}: {error}")))?;
    if bytes.len() as u64 > policy.max_observation_file_bytes {
        return Err(WorkspaceError::Limit(format!(
            "workspace file exceeds observation limit: {path} ({} > {})",
            bytes.len(),
            policy.max_observation_file_bytes
        )));
    }
    Ok((target, NormalizedText::decode(bytes)?))
}

pub(crate) fn edit_result(
    context: &MutationContext,
    changes: Vec<FileChange>,
    post_edit_sources: Vec<PostEditSource>,
    matching_strategy: Option<String>,
    confidence: Option<f64>,
) -> EditResult {
    EditResult {
        mode: context.mode.clone(),
        mode_generation: context.mode_generation,
        policy_generation: context.policy_generation,
        changes,
        post_edit_sources,
        preflight_complete: true,
        commit_complete: true,
        matching_strategy,
        confidence,
    }
}
