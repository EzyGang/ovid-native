use std::collections::HashSet;
use std::fs;
use std::ops::Range;

use crate::workspace::content::NormalizedText;
use crate::workspace::path::{resolve_contained_file, validate_relative};
use crate::workspace::{
    EditResult, FileChange, LineRange, ObservationReceipt, PolicyGeneration, PostEditSource,
    Workspace, WorkspaceError, WorkspacePolicy, atomic_replace_path, create_file, sha256,
};

#[derive(Clone, Debug)]
pub(crate) struct MutationContext {
    pub mode: String,
    pub mode_generation: u64,
    pub policy_generation: u64,
    pub policy: WorkspacePolicy,
}

impl MutationContext {
    pub(crate) fn write(policy: PolicyGeneration) -> Self {
        Self {
            mode: "write".to_owned(),
            mode_generation: 1,
            policy_generation: policy.generation,
            policy: policy.policy,
        }
    }
}

impl Workspace {
    pub(crate) fn resolve_observation(
        &self,
        path: &str,
        tag: &str,
    ) -> Result<ObservationReceipt, WorkspaceError> {
        let policy = self.policy()?;
        let (_, text) = load_current(self, path, &policy.policy)?;
        let digest = sha256(text.source.as_bytes());
        Ok(self.observations()?.resolve(path, tag, &digest)?.receipt)
    }

    pub(crate) fn validate_observed_lines(
        &self,
        path: &str,
        tag: &str,
        lines: &[usize],
    ) -> Result<ObservationReceipt, WorkspaceError> {
        let policy = self.policy()?;
        let (_, text) = load_current(self, path, &policy.policy)?;
        let digest = sha256(text.source.as_bytes());
        let authorization = self.observations()?.resolve(path, tag, &digest)?;
        authorization.require_lines(path, &text, &lines.iter().copied().collect())?;
        Ok(authorization.receipt)
    }

    pub(crate) fn create_text_file(
        &self,
        path: &str,
        content: &str,
        create_parents: bool,
        context: &MutationContext,
    ) -> Result<EditResult, WorkspaceError> {
        validate_relative(path)?;
        let _coordinator = self.write_guard()?;
        let text = NormalizedText::from_replacement(content);
        let parents_allowed = create_parents && context.policy.create_parent_directories;
        if create_parents && !parents_allowed {
            return Err(WorkspaceError::Write(format!(
                "workspace policy does not allow parent creation: {path}"
            )));
        }
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
        let (target, current) = load_current(self, path, &context.policy)?;
        let before = sha256(current.source.as_bytes());
        let authorization = self
            .observations()?
            .resolve(path, expected_observation, &before)?;
        authorization.require_complete(path)?;
        let replacement = NormalizedText::from_replacement(content);
        atomic_replace_path(&target, path, &replacement.serialize())?;
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

    pub(crate) fn replace_text(
        &self,
        path: &str,
        old_string: &str,
        new_string: &str,
        replace_all: bool,
        context: &MutationContext,
    ) -> Result<EditResult, WorkspaceError> {
        if old_string.is_empty() {
            return Err(WorkspaceError::Patch(
                "replace old string must be non-empty".to_owned(),
            ));
        }
        let _coordinator = self.write_guard()?;
        let (target, current) = load_current(self, path, &context.policy)?;
        let before = sha256(current.source.as_bytes());
        let authorization = self.observations()?.current(path, &before)?;
        let old = NormalizedText::from_replacement(old_string).source;
        let new = NormalizedText::from_replacement(new_string).source;
        let exact = current
            .source
            .match_indices(&old)
            .map(|(start, _)| start..start + old.len())
            .collect::<Vec<_>>();
        let (matches, strategy, confidence) = if exact.is_empty() {
            fuzzy_matches(&current, &old, &context.policy)?
        } else {
            (exact, "exact".to_owned(), 1.0)
        };
        if !replace_all && matches.len() != 1 {
            return Err(WorkspaceError::Patch(if matches.is_empty() {
                format!("replace text was not found: {path}")
            } else {
                format!("replace text is ambiguous: {path}")
            }));
        }
        if matches.is_empty() {
            return Err(WorkspaceError::Patch(format!(
                "replace text was not found: {path}"
            )));
        }
        let selected = if replace_all {
            &matches[..]
        } else {
            &matches[..1]
        };
        let required = selected
            .iter()
            .flat_map(|range| source_lines(&current.source, range.clone()))
            .collect::<HashSet<_>>();
        authorization.require_lines(path, &current, &required)?;
        let source = apply_replacements(&current.source, selected, &new);
        let replacement = NormalizedText::from_replacement(&source);
        atomic_replace_path(
            &target,
            path,
            &current.serialize_with_current(&replacement.source),
        )?;
        let (file_generation, revision) = self.mark_file_changed(path)?;
        let after = sha256(replacement.source.as_bytes());
        let changed_lines =
            changed_line_range(&current.source, selected, &new, replacement.total_lines());
        let post = self.record_post_edit(
            path,
            &replacement,
            file_generation,
            changed_lines,
            &context.policy,
        )?;
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
            Some(strategy),
            Some(confidence),
        ))
    }

    pub(crate) fn delete_text_file(
        &self,
        path: &str,
        context: &MutationContext,
    ) -> Result<EditResult, WorkspaceError> {
        let _coordinator = self.write_guard()?;
        let (target, current) = load_current(self, path, &context.policy)?;
        let before = sha256(current.source.as_bytes());
        self.observations()?
            .current(path, &before)?
            .require_complete(path)?;
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
        let destination_path =
            crate::workspace::path::resolve_new_file(self.root(), destination, false)?;
        fs::rename(&target, &destination_path).map_err(|error| {
            WorkspaceError::Write(format!("cannot move {path} to {destination}: {error}"))
        })?;
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

fn source_lines(source: &str, range: Range<usize>) -> HashSet<usize> {
    let start = source[..range.start]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
        + 1;
    let end_offset = range.end.saturating_sub(1).max(range.start);
    let end = source[..end_offset]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
        + 1;
    (start..=end).collect()
}

fn apply_replacements(source: &str, matches: &[Range<usize>], replacement: &str) -> String {
    let capacity = source
        .len()
        .saturating_add(replacement.len().saturating_mul(matches.len()));
    let mut result = String::with_capacity(capacity);
    let mut cursor = 0;
    for range in matches {
        result.push_str(&source[cursor..range.start]);
        result.push_str(replacement);
        cursor = range.end;
    }
    result.push_str(&source[cursor..]);
    result
}

fn changed_line_range(
    source: &str,
    matches: &[Range<usize>],
    replacement: &str,
    total_lines: usize,
) -> Option<LineRange> {
    let first = matches.first()?;
    let last = matches.last()?;
    let start = source[..first.start]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
        + 1;
    let replaced_lines = replacement.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let removed_lines = source[first.start..last.end]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
        + 1;
    let end = start
        .saturating_add(replaced_lines.max(removed_lines))
        .saturating_add(2)
        .min(total_lines);
    Some(LineRange {
        start: start.saturating_sub(2).max(1),
        end,
    })
}

fn fuzzy_matches(
    current: &NormalizedText,
    old: &str,
    policy: &WorkspacePolicy,
) -> Result<(Vec<Range<usize>>, String, f64), WorkspaceError> {
    if !policy.allow_fuzzy_replace {
        return Ok((Vec::new(), "exact".to_owned(), 0.0));
    }
    let line_count = old.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let current_lines = (1..=current.total_lines())
        .filter_map(|number| current.line(number))
        .collect::<Vec<_>>();
    if line_count > current_lines.len() {
        return Ok((Vec::new(), "fuzzy".to_owned(), 0.0));
    }
    let mut candidates = Vec::new();
    for start in 0..=current_lines.len() - line_count {
        let candidate = current_lines[start..start + line_count].join("\n");
        let confidence = similarity(old, &candidate);
        if confidence >= policy.fuzzy_replace_threshold
            && let Some(offset) = current.source.find(&candidate)
        {
            candidates.push((offset..offset + candidate.len(), confidence));
        }
    }
    candidates.sort_by(|left, right| right.1.total_cmp(&left.1));
    if candidates.len() > 1 && (candidates[0].1 - candidates[1].1).abs() < f64::EPSILON {
        return Err(WorkspaceError::Patch(
            "fuzzy replace candidate is ambiguous".to_owned(),
        ));
    }
    Ok(match candidates.first() {
        Some((range, confidence)) => (vec![range.clone()], "fuzzy".to_owned(), *confidence),
        None => (Vec::new(), "fuzzy".to_owned(), 0.0),
    })
}

fn similarity(left: &str, right: &str) -> f64 {
    let maximum = left.chars().count().max(right.chars().count());
    if maximum == 0 {
        return 1.0;
    }
    1.0 - levenshtein(left, right) as f64 / maximum as f64
}

fn levenshtein(left: &str, right: &str) -> usize {
    let right = right.chars().collect::<Vec<_>>();
    let mut previous = (0..=right.len()).collect::<Vec<_>>();
    for (left_index, left_character) in left.chars().enumerate() {
        let mut current = vec![left_index + 1];
        for (right_index, right_character) in right.iter().enumerate() {
            current.push(
                (previous[right_index + 1] + 1)
                    .min(current[right_index] + 1)
                    .min(previous[right_index] + usize::from(left_character != *right_character)),
            );
        }
        previous = current;
    }
    previous[right.len()]
}
