use ast_grep_core::meta_var::MetaVariable;
use ast_grep_language::SupportLang;

use crate::ast::{
    language::canonical_name,
    source_range,
    types::{Capture, Match},
};
use crate::workspace::WorkspaceEntry;

pub(crate) fn build_match<D>(
    candidate: &WorkspaceEntry,
    language: SupportLang,
    source: &str,
    matched: &ast_grep_core::NodeMatch<'_, D>,
    include_captures: bool,
) -> Match
where
    D: ast_grep_core::Doc,
{
    let byte_range = matched.range();
    let captures = if include_captures {
        captures(source, matched)
    } else {
        Vec::new()
    };
    let range = source_range(source, byte_range.clone());
    let source_lines = matched_source_lines(source, &range);
    (
        candidate.relative.clone(),
        canonical_name(language).to_owned(),
        source[byte_range].to_owned(),
        range,
        captures,
        source_lines,
    )
}

fn matched_source_lines(source: &str, range: &crate::ast::types::Range) -> Vec<(usize, String)> {
    let start = range.0.0;
    let end = if range.1.1 == 1 && range.1.0 > start {
        range.1.0 - 1
    } else {
        range.1.0
    };
    source
        .lines()
        .enumerate()
        .filter(|(index, _)| (start..=end).contains(&(index + 1)))
        .map(|(index, line)| (index + 1, line.to_owned()))
        .collect()
}

fn captures<D>(source: &str, matched: &ast_grep_core::NodeMatch<'_, D>) -> Vec<Capture>
where
    D: ast_grep_core::Doc,
{
    let environment = matched.get_env();
    let mut captures = environment
        .get_matched_variables()
        .filter_map(|variable| match variable {
            MetaVariable::Capture(name, _) => environment.get_match(&name).map(|node| {
                let range = node.range();
                (
                    name,
                    source[range.clone()].to_owned(),
                    Some(source_range(source, range)),
                )
            }),
            MetaVariable::MultiCapture(name) => {
                let nodes = environment.get_multiple_matches(&name);
                let range = nodes.first().zip(nodes.last()).map(|(first, last)| {
                    let range = first.range().start..last.range().end;
                    (
                        source[range.clone()].to_owned(),
                        source_range(source, range),
                    )
                });
                Some(match range {
                    Some((text, range)) => (name, text, Some(range)),
                    None => (name, String::new(), None),
                })
            }
            MetaVariable::Dropped(_) | MetaVariable::Multiple => None,
        })
        .collect::<Vec<_>>();
    captures.sort_by(|left, right| left.0.cmp(&right.0));
    captures
}
