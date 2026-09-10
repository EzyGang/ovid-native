use std::collections::HashSet;

use crate::workspace::text_similarity::similarity;
use crate::workspace::{LineRange, NormalizedText, WorkspaceError};

pub(super) fn apply_unified_diff(
    current: &NormalizedText,
    diff: &str,
    allow_fuzzy: bool,
    threshold: f64,
) -> Result<(String, HashSet<usize>, Option<LineRange>), WorkspaceError> {
    let hunks = parse_hunks(diff)?;
    let source_lines = (1..=current.total_lines())
        .filter_map(|number| current.line(number).map(str::to_owned))
        .collect::<Vec<_>>();
    let mut matches = Vec::with_capacity(hunks.len());
    let mut required = HashSet::new();
    for hunk in &hunks {
        let old = hunk
            .lines
            .iter()
            .filter(|line| line.kind != '+')
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>();
        let positions = exact_positions(&source_lines, &old);
        let start = match positions.as_slice() {
            [position] => *position,
            [] if allow_fuzzy => fuzzy_position(&source_lines, &old, threshold)?,
            [] => {
                return Err(WorkspaceError::Patch(
                    "patch context was not found".to_owned(),
                ));
            }
            _ => {
                return Err(WorkspaceError::Patch(
                    "patch context is ambiguous".to_owned(),
                ));
            }
        };
        if old.is_empty() {
            let anchor = start.min(source_lines.len().saturating_sub(1)) + 1;
            required.insert(anchor);
        } else {
            required.extend((start + 1)..=(start + old.len()));
        }
        matches.push((start, old.len(), hunk));
    }
    matches.sort_by_key(|(start, _, _)| *start);
    for windows in matches.windows(2) {
        if windows[0].0 + windows[0].1 > windows[1].0 {
            return Err(WorkspaceError::Patch("patch hunks overlap".to_owned()));
        }
    }
    let changed_start = matches.first().map(|(start, _, _)| start + 1);
    let mut result = source_lines;
    for (start, old_length, hunk) in matches.into_iter().rev() {
        let replacement = hunk
            .lines
            .iter()
            .filter(|line| line.kind != '-')
            .map(|line| line.text.clone())
            .collect::<Vec<_>>();
        result.splice(start..start + old_length, replacement);
    }
    let mut source = result.join("\n");
    if current.source.ends_with('\n') {
        source.push('\n');
    }
    let changed = changed_start.map(|start| LineRange {
        start: start.saturating_sub(2).max(1),
        end: start.saturating_add(4).min(result.len()),
    });
    Ok((source, required, changed))
}

#[derive(Debug)]
struct HunkLine {
    kind: char,
    text: String,
}

#[derive(Debug)]
struct Hunk {
    lines: Vec<HunkLine>,
}

fn parse_hunks(diff: &str) -> Result<Vec<Hunk>, WorkspaceError> {
    let normalized = diff.replace("\r\n", "\n").replace('\r', "\n");
    let mut hunks = Vec::new();
    let mut current: Option<Vec<HunkLine>> = None;
    for line in normalized.lines() {
        if line.starts_with("@@") {
            if let Some(lines) = current.take() {
                validate_hunk(&lines)?;
                hunks.push(Hunk { lines });
            }
            current = Some(Vec::new());
            continue;
        }
        let lines = current.as_mut().ok_or_else(|| {
            WorkspaceError::Patch("patch diff must begin with an @@ hunk header".to_owned())
        })?;
        let kind = line
            .chars()
            .next()
            .ok_or_else(|| WorkspaceError::Patch("patch hunk contains an empty row".to_owned()))?;
        if !matches!(kind, ' ' | '+' | '-') {
            return Err(WorkspaceError::Patch(
                "patch hunk rows must begin with space, '+', or '-'".to_owned(),
            ));
        }
        lines.push(HunkLine {
            kind,
            text: line[1..].to_owned(),
        });
    }
    if let Some(lines) = current {
        validate_hunk(&lines)?;
        hunks.push(Hunk { lines });
    }
    if hunks.is_empty() {
        return Err(WorkspaceError::Patch(
            "patch diff must contain at least one hunk".to_owned(),
        ));
    }
    Ok(hunks)
}

fn validate_hunk(lines: &[HunkLine]) -> Result<(), WorkspaceError> {
    if !lines.iter().any(|line| matches!(line.kind, '+' | '-')) {
        return Err(WorkspaceError::Patch(
            "every patch hunk must contain a change".to_owned(),
        ));
    }
    Ok(())
}

fn exact_positions(source: &[String], pattern: &[&str]) -> Vec<usize> {
    if pattern.is_empty() {
        return Vec::new();
    }
    source
        .windows(pattern.len())
        .enumerate()
        .filter(|(_, candidate)| {
            candidate
                .iter()
                .zip(pattern)
                .all(|(candidate, expected)| candidate == expected)
        })
        .map(|(index, _)| index)
        .collect()
}

fn fuzzy_position(
    source: &[String],
    pattern: &[&str],
    threshold: f64,
) -> Result<usize, WorkspaceError> {
    if pattern.is_empty() || pattern.len() > source.len() {
        return Err(WorkspaceError::Patch(
            "fuzzy patch context was not found".to_owned(),
        ));
    }
    let expected = pattern.join("\n");
    let mut candidates = source
        .windows(pattern.len())
        .enumerate()
        .map(|(index, lines)| (index, similarity(&expected, &lines.join("\n"))))
        .filter(|(_, confidence)| *confidence >= threshold)
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| right.1.total_cmp(&left.1));
    match candidates.as_slice() {
        [] => Err(WorkspaceError::Patch(
            "fuzzy patch context was not found".to_owned(),
        )),
        [candidate] => Ok(candidate.0),
        [first, second, ..] if first.1 > second.1 => Ok(first.0),
        _ => Err(WorkspaceError::Patch(
            "fuzzy patch context is ambiguous".to_owned(),
        )),
    }
}
