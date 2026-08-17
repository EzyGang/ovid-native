use std::collections::HashSet;
use std::ops::Range;

use crate::workspace::content::NormalizedText;
use crate::workspace::path::normalize_relative;
use crate::workspace::workflows::{edit_result, load_current};
use crate::workspace::{
    EditResult, FileChange, LineRange, MutationContext, Workspace, WorkspaceError, WorkspacePolicy,
    atomic_replace_path, sha256,
};

impl Workspace {
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
        let path = normalize_relative(path)?;
        let _coordinator = self.write_guard()?;
        self.validate_mutation(context, "replace")?;

        let (target, current) = load_current(self, &path, &context.policy)?;
        let before = sha256(current.source.as_bytes());
        let authorization = self.observations()?.current(&path, &before)?;
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
        authorization.require_lines(&path, &current, &required)?;
        let source = apply_replacements(&current.source, selected, &new);
        let replacement = NormalizedText::from_replacement(&source);
        context.ensure_active()?;

        atomic_replace_path(
            &target,
            &path,
            &before,
            &current.serialize_with_current(&replacement.source),
        )?;
        let (file_generation, revision) = self.mark_file_changed(&path)?;
        let after = sha256(replacement.source.as_bytes());
        let changed_lines =
            changed_line_range(&current.source, selected, &new, replacement.total_lines());
        let post = self.record_post_edit(
            &path,
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
