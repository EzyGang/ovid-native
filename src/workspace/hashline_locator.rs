use std::collections::HashSet;

use crate::workspace::hashline_block::resolve_syntax_block;
use crate::workspace::hashline_types::{
    HashlineContent, HashlineOperation, ResolvedHashlineOperation,
};
use crate::workspace::line_hash::short_line_hash;
use crate::workspace::observation_types::Authorization;
use crate::workspace::{NormalizedText, WorkspaceError};

pub(crate) fn resolve_operations(
    path: &str,
    text: &NormalizedText,
    authorization: &Authorization,
    operations: &[HashlineOperation],
) -> Result<Vec<ResolvedHashlineOperation>, WorkspaceError> {
    let mut resolved = Vec::new();
    let mut destructive = Vec::new();

    for (order, operation) in operations.iter().enumerate() {
        let item = resolve_operation(path, text, authorization, operation, order)?;
        if let Some(range) = destructive_range(&item) {
            if destructive.iter().any(|other| overlaps(range, *other)) {
                return Err(WorkspaceError::Patch(format!(
                    "Hashline destructive locators overlap: {path}"
                )));
            }
            destructive.push(range);
        }
        resolved.push(item);
    }

    Ok(resolved)
}

fn resolve_operation(
    path: &str,
    text: &NormalizedText,
    authorization: &Authorization,
    operation: &HashlineOperation,
    order: usize,
) -> Result<ResolvedHashlineOperation, WorkspaceError> {
    match operation.kind.as_str() {
        "put_range" => {
            let (start, end) = range(path, text, authorization, operation)?;
            Ok(put(operation, start - 1, Some((start, end)), order))
        }
        "cut_range" => {
            let (start, end) = range(path, text, authorization, operation)?;
            Ok(cut(operation, start, end, order))
        }
        "put_block" | "cut_block" | "put_after_block" => {
            resolve_block(path, text, authorization, operation, order)
        }
        "put_before" | "put_after" => {
            let line = anchor(path, text, authorization, operation)?;
            let gap = if operation.kind == "put_before" {
                line - 1
            } else {
                line
            };
            Ok(put(operation, gap, None, order))
        }
        "put_begin" | "put_end" => boundary(path, text, authorization, operation, order),
        _ => Err(WorkspaceError::Patch(format!(
            "unsupported Hashline operation: {}",
            operation.kind
        ))),
    }
}

fn range(
    path: &str,
    text: &NormalizedText,
    authorization: &Authorization,
    operation: &HashlineOperation,
) -> Result<(usize, usize), WorkspaceError> {
    let start = required_line(operation.start, path)?;
    let end = required_line(operation.end, path)?;
    if start > end {
        return Err(WorkspaceError::Patch(format!(
            "Hashline range is reversed: {path}:{start}-{end}"
        )));
    }
    validate_hash(path, text, start, operation.start_hash.as_deref())?;
    validate_hash(path, text, end, operation.end_hash.as_deref())?;
    require_lines(path, text, authorization, start, end)?;
    Ok((start, end))
}

fn resolve_block(
    path: &str,
    text: &NormalizedText,
    authorization: &Authorization,
    operation: &HashlineOperation,
    order: usize,
) -> Result<ResolvedHashlineOperation, WorkspaceError> {
    let start = anchor(path, text, authorization, operation)?;
    let (block_start, block_end) = resolve_syntax_block(path, &text.source, start)?;
    require_lines(path, text, authorization, block_start, block_end)?;

    match operation.kind.as_str() {
        "cut_block" => Ok(cut(operation, block_start, block_end, order)),
        "put_after_block" => Ok(put(operation, block_end, None, order)),
        _ => Ok(put(
            operation,
            block_start - 1,
            Some((block_start, block_end)),
            order,
        )),
    }
}

fn boundary(
    path: &str,
    text: &NormalizedText,
    authorization: &Authorization,
    operation: &HashlineOperation,
    order: usize,
) -> Result<ResolvedHashlineOperation, WorkspaceError> {
    if text.total_lines() == 0 {
        authorization.require_complete(path)?;
    } else {
        let line = if operation.kind == "put_begin" {
            1
        } else {
            text.total_lines()
        };
        require_lines(path, text, authorization, line, line)?;
    }
    let gap = if operation.kind == "put_begin" {
        0
    } else {
        text.total_lines()
    };
    Ok(put(operation, gap, None, order))
}

fn anchor(
    path: &str,
    text: &NormalizedText,
    authorization: &Authorization,
    operation: &HashlineOperation,
) -> Result<usize, WorkspaceError> {
    let line = required_line(operation.start, path)?;
    validate_hash(path, text, line, operation.start_hash.as_deref())?;
    require_lines(path, text, authorization, line, line)?;
    Ok(line)
}

fn validate_hash(
    path: &str,
    text: &NormalizedText,
    line: usize,
    expected: Option<&str>,
) -> Result<(), WorkspaceError> {
    let source = text.line(line).ok_or_else(|| {
        WorkspaceError::ObservedLineChanged(format!(
            "Hashline anchor no longer exists; reread {path}:{line}"
        ))
    })?;
    let actual = format!("{:02X}", short_line_hash(source.as_bytes()));
    if expected.is_some_and(|value| !value.eq_ignore_ascii_case(&actual)) {
        return Err(WorkspaceError::ObservedLineChanged(format!(
            "Hashline anchor hash changed; reread {path}:{line}"
        )));
    }
    Ok(())
}

fn require_lines(
    path: &str,
    text: &NormalizedText,
    authorization: &Authorization,
    start: usize,
    end: usize,
) -> Result<(), WorkspaceError> {
    authorization.require_lines(path, text, &(start..=end).collect::<HashSet<_>>())
}

fn required_line(value: Option<usize>, path: &str) -> Result<usize, WorkspaceError> {
    value.filter(|line| *line > 0).ok_or_else(|| {
        WorkspaceError::Patch(format!("Hashline locator is missing a line number: {path}"))
    })
}

fn put(
    operation: &HashlineOperation,
    gap: usize,
    remove: Option<(usize, usize)>,
    order: usize,
) -> ResolvedHashlineOperation {
    let content = match operation.register.as_ref() {
        Some(register) => HashlineContent::Register(Some(register.clone())),
        None if operation.body.is_empty() => HashlineContent::Register(None),
        None => HashlineContent::Body(operation.body.clone()),
    };
    ResolvedHashlineOperation::Put {
        gap,
        remove,
        content,
        order,
    }
}

fn cut(
    operation: &HashlineOperation,
    start: usize,
    end: usize,
    order: usize,
) -> ResolvedHashlineOperation {
    ResolvedHashlineOperation::Cut {
        start,
        end,
        register: operation.register.clone(),
        order,
    }
}

fn destructive_range(operation: &ResolvedHashlineOperation) -> Option<(usize, usize)> {
    match operation {
        ResolvedHashlineOperation::Put {
            remove: Some(range),
            ..
        } => Some(*range),
        ResolvedHashlineOperation::Cut { start, end, .. } => Some((*start, *end)),
        ResolvedHashlineOperation::Put { remove: None, .. } => None,
    }
}

fn overlaps(left: (usize, usize), right: (usize, usize)) -> bool {
    left.0 <= right.1 && right.0 <= left.1
}
