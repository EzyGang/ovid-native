use std::path::Path;

use ast_grep_language::{Language, LanguageExt, SupportLang};

use crate::workspace::WorkspaceError;

pub(crate) fn resolve_syntax_block(
    path: &str,
    source: &str,
    anchor: usize,
) -> Result<(usize, usize), WorkspaceError> {
    let language = SupportLang::from_path(Path::new(path)).ok_or_else(|| {
        WorkspaceError::Patch(format!(
            "Hashline syntax blocks are unsupported for this file: {path}:{anchor}"
        ))
    })?;
    let tree = language.ast_grep(source);
    if tree
        .root()
        .dfs()
        .any(|node| node.is_error() || node.is_missing())
    {
        return Err(WorkspaceError::Patch(format!(
            "Hashline syntax block cannot resolve a file with parse errors: {path}:{anchor}"
        )));
    }

    let mut candidates = tree
        .root()
        .dfs()
        .filter(|node| {
            node.parent().is_some()
                && node.is_named()
                && node.start_pos().line().saturating_add(1) == anchor
                && node.end_pos().line() > node.start_pos().line()
        })
        .map(|node| {
            let end = if node.end_pos().byte_point().1 == 0 {
                node.end_pos().line()
            } else {
                node.end_pos().line().saturating_add(1)
            };
            (anchor, end, node.range().len())
        })
        .collect::<Vec<_>>();
    candidates.sort_unstable_by_key(|(_, _, bytes)| std::cmp::Reverse(*bytes));

    let Some((start, end, largest)) = candidates.first().copied() else {
        return Err(WorkspaceError::Patch(format!(
            "Hashline locator does not begin an eligible syntax block: {path}:{anchor}"
        )));
    };
    if candidates
        .iter()
        .skip(1)
        .any(|(_, candidate_end, bytes)| *bytes == largest && *candidate_end != end)
    {
        return Err(WorkspaceError::Patch(format!(
            "Hashline syntax block is ambiguous: {path}:{anchor}"
        )));
    }

    Ok((start, end.max(start)))
}
