use std::collections::HashMap;

use crate::workspace::hashline_types::{
    HashlineContent, HashlineRegister, PreparedHashlineSection, ResolvedHashlineOperation,
};
use crate::workspace::{LineRange, WorkspaceError};

pub(crate) fn apply_operations(
    section: &mut PreparedHashlineSection,
    named: &mut HashMap<String, HashlineRegister>,
    anonymous: &mut Option<HashlineRegister>,
) -> Result<(), WorkspaceError> {
    let trailing_newline = section.file.current.source.ends_with('\n');
    let original = source_lines(&section.file.current);
    let mut edits: Vec<PendingEdit> = Vec::new();
    for operation in &section.operations {
        match operation {
            ResolvedHashlineOperation::Put {
                gap,
                remove,
                content,
                order,
            } => {
                let (body, register_trailing_newline) = resolve_content(content, named, anonymous)?;
                edits.push((*gap, *remove, body, register_trailing_newline, *order));
            }
            ResolvedHashlineOperation::Cut {
                start,
                end,
                register,
                order,
            } => {
                let value = HashlineRegister {
                    lines: original[*start - 1..*end].to_vec(),
                    trailing_newline: *end < original.len() || trailing_newline,
                };
                let cut_trailing_newline =
                    (*end == original.len() && !trailing_newline && *start > 1).then_some(true);
                match register {
                    Some(name) => {
                        named.insert(name.clone(), value);
                    }
                    None => *anonymous = Some(value),
                }
                edits.push((
                    *start - 1,
                    Some((*start, *end)),
                    Vec::new(),
                    cut_trailing_newline,
                    *order,
                ));
            }
        }
    }
    section.file.final_source = apply_edits(original, edits, trailing_newline);
    section.file.changed_range =
        changed_range(&section.file.current.source, &section.file.final_source);
    Ok(())
}

type PendingEdit = (
    usize,
    Option<(usize, usize)>,
    Vec<String>,
    Option<bool>,
    usize,
);

fn apply_edits(
    mut lines: Vec<String>,
    mut edits: Vec<PendingEdit>,
    mut trailing_newline: bool,
) -> String {
    edits.sort_unstable_by_key(|(gap, _, _, _, order)| {
        (std::cmp::Reverse(*gap), std::cmp::Reverse(*order))
    });
    for (gap, remove, body, register_trailing_newline, _) in edits {
        let range = remove.map_or(gap..gap, |(start, end)| start - 1..end);
        let replaces_tail = range.end == lines.len();
        lines.splice(range, body);
        if replaces_tail && let Some(value) = register_trailing_newline {
            trailing_newline = value;
        }
    }
    join_lines(&lines, trailing_newline)
}

fn resolve_content(
    content: &HashlineContent,
    named: &HashMap<String, HashlineRegister>,
    anonymous: &Option<HashlineRegister>,
) -> Result<(Vec<String>, Option<bool>), WorkspaceError> {
    match content {
        HashlineContent::Body(lines) => Ok((lines.clone(), None)),
        HashlineContent::Register(Some(name)) => named
            .get(name)
            .map(|value| (value.lines.clone(), Some(value.trailing_newline)))
            .ok_or_else(|| {
                WorkspaceError::Patch(format!("Hashline register is unavailable: @{name}"))
            }),
        HashlineContent::Register(None) => anonymous
            .as_ref()
            .map(|value| (value.lines.clone(), Some(value.trailing_newline)))
            .ok_or_else(|| {
                WorkspaceError::Patch("anonymous Hashline register is unavailable".to_owned())
            }),
    }
}

fn source_lines(text: &crate::workspace::NormalizedText) -> Vec<String> {
    (1..=text.total_lines())
        .filter_map(|line| text.line(line).map(str::to_owned))
        .collect()
}

fn join_lines(lines: &[String], trailing_newline: bool) -> String {
    let mut source = lines.join("\n");
    if trailing_newline && !lines.is_empty() {
        source.push('\n');
    }
    source
}

fn changed_range(before: &str, after: &str) -> Option<LineRange> {
    if before == after {
        return None;
    }
    let before_lines = before.lines().collect::<Vec<_>>();
    let after_lines = after.lines().collect::<Vec<_>>();
    let prefix = before_lines
        .iter()
        .zip(&after_lines)
        .take_while(|(left, right)| left == right)
        .count();
    let suffix = before_lines[prefix..]
        .iter()
        .rev()
        .zip(after_lines[prefix..].iter().rev())
        .take_while(|(left, right)| left == right)
        .count();
    let end = after_lines.len().saturating_sub(suffix);
    let line = prefix.saturating_add(1).min(after_lines.len().max(1));
    Some(LineRange {
        start: line,
        end: end.max(line),
    })
}
