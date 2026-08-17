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
    let original = source_lines(&section.file.current);
    let mut edits = Vec::new();
    for operation in &section.operations {
        match operation {
            ResolvedHashlineOperation::Put {
                gap,
                remove,
                content,
                order,
            } => edits.push((
                *gap,
                *remove,
                resolve_content(content, named, anonymous)?,
                *order,
            )),
            ResolvedHashlineOperation::Cut {
                start,
                end,
                register,
                order,
            } => {
                let value = HashlineRegister {
                    lines: original[*start - 1..*end].to_vec(),
                    ended_at_file_end: *end == original.len(),
                };
                match register {
                    Some(name) => {
                        named.insert(name.clone(), value);
                    }
                    None => *anonymous = Some(value),
                }
                edits.push((*start - 1, Some((*start, *end)), Vec::new(), *order));
            }
        }
    }
    edits.sort_unstable_by_key(|(gap, _, _, order)| {
        (std::cmp::Reverse(*gap), std::cmp::Reverse(*order))
    });

    let mut final_lines = original;
    for (gap, remove, body, _) in edits {
        let range = remove.map_or(gap..gap, |(start, end)| start - 1..end);
        final_lines.splice(range, body);
    }
    section.file.final_source =
        join_lines(&final_lines, section.file.current.source.ends_with('\n'));
    section.file.changed_range =
        changed_range(&section.file.current.source, &section.file.final_source);
    Ok(())
}

fn resolve_content(
    content: &HashlineContent,
    named: &HashMap<String, HashlineRegister>,
    anonymous: &Option<HashlineRegister>,
) -> Result<Vec<String>, WorkspaceError> {
    match content {
        HashlineContent::Body(lines) => Ok(lines.clone()),
        HashlineContent::Register(Some(name)) => named
            .get(name)
            .map(|value| value.lines.clone())
            .ok_or_else(|| {
                WorkspaceError::Patch(format!("Hashline register is unavailable: @{name}"))
            }),
        HashlineContent::Register(None) => anonymous
            .as_ref()
            .map(|value| value.lines.clone())
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
