use std::collections::HashMap;

use ast_grep_core::Pattern;
use ast_grep_core::meta_var::MetaVariable;
use ast_grep_language::{LanguageExt, SupportLang};

use crate::ast::language::{canonical_name, infer_language, resolve_language, strictness};
use crate::ast::types::{Capture, Issue, Match, ScanOptions, SearchRequest, SearchResult};
use crate::ast::{AstError, source_range};
use crate::workspace::{
    MetadataLevel, ReadExtent, ScanFileKind, ScanOrder, ScanRequest, WorkCompletion, WorkControl,
    Workspace, WorkspaceEntry, read_content,
};

pub fn search(workspace: &Workspace, request: SearchRequest) -> Result<SearchResult, AstError> {
    if request.pattern.is_empty() || request.pattern.contains('\0') {
        return Err(AstError::Pattern(
            "AST patterns must be non-empty and contain no NUL bytes".to_owned(),
        ));
    }
    if request.limit == 0 {
        return Err(AstError::Configuration(
            "search limit must be positive".to_owned(),
        ));
    }
    let explicit = request
        .language
        .as_deref()
        .map(resolve_language)
        .transpose()?;
    let strictness = strictness(&request.strictness)?;
    let selected = discover_files(
        workspace,
        &request.scan,
        request.limits.max_files,
        &request.cancellation,
    )?;
    let (files, unsupported_files) = classify_files(selected, explicit);
    let mut patterns = HashMap::new();
    for (_, language) in &files {
        patterns.entry(*language).or_insert_with(|| {
            Pattern::try_new(&request.pattern, *language)
                .map(|pattern| pattern.with_strictness(strictness.clone()))
        });
    }
    let patterns = patterns
        .into_iter()
        .map(|(language, result)| {
            result
                .map(|pattern| (language, pattern))
                .map_err(|error| AstError::Pattern(error.to_string()))
        })
        .collect::<Result<HashMap<_, _>, _>>()?;

    let mut matches = Vec::new();
    let mut issues = Vec::new();
    let mut total_matches = 0;
    let mut files_searched = 0;
    let mut files_with_matches = 0;
    let mut limit_reached = false;

    'files: for (candidate, language) in files {
        if request.cancellation.is_cancelled() {
            return Err(AstError::Cancelled);
        }
        let source = match read_source(
            &candidate,
            request.limits.max_file_bytes,
            &request.cancellation,
        ) {
            Ok(source) => source,
            Err(issue) => {
                if request.cancellation.is_cancelled() {
                    return Err(AstError::Cancelled);
                }
                issues.push(issue);
                continue;
            }
        };
        let tree = language.ast_grep(&source);
        if tree
            .root()
            .dfs()
            .any(|node| node.is_error() || node.is_missing())
        {
            issues.push(parse_issue(&candidate, language));
            continue;
        }
        files_searched += 1;
        let before_file = total_matches;
        let pattern = &patterns[&language];
        for matched in tree.root().find_all(pattern) {
            if request.cancellation.is_cancelled() {
                return Err(AstError::Cancelled);
            }
            if total_matches == request.limits.max_matches {
                limit_reached = true;
                issues.push(limit_issue(request.limits.max_matches));
                if total_matches > before_file {
                    files_with_matches += 1;
                }
                break 'files;
            }
            if total_matches >= request.offset && matches.len() < request.limit {
                matches.push(build_match(
                    &candidate,
                    language,
                    &source,
                    &matched,
                    request.include_captures,
                ));
            }
            total_matches += 1;
        }
        if total_matches > before_file {
            files_with_matches += 1;
        }
    }
    let truncated = limit_reached || total_matches.saturating_sub(request.offset) > matches.len();
    Ok((
        matches,
        total_matches,
        files_searched,
        files_with_matches,
        unsupported_files,
        truncated,
        issues,
    ))
}

pub(crate) fn classify_files(
    selected: Vec<WorkspaceEntry>,
    explicit: Option<SupportLang>,
) -> (Vec<(WorkspaceEntry, SupportLang)>, usize) {
    let mut files = Vec::new();
    let mut unsupported = 0;
    for candidate in selected {
        match explicit.or_else(|| infer_language(&candidate.path)) {
            Some(language) => files.push((candidate, language)),
            None => unsupported += 1,
        }
    }
    (files, unsupported)
}

pub(crate) fn discover_files(
    workspace: &Workspace,
    options: &ScanOptions,
    max_files: usize,
    cancellation: &crate::workspace::Cancellation,
) -> Result<Vec<WorkspaceEntry>, AstError> {
    let control = WorkControl::new(cancellation.clone(), None);
    let result = workspace.scan(
        &ScanRequest {
            selections: options.paths.clone(),
            include_hidden: options.include_hidden,
            respect_gitignore: options.respect_gitignore,
            include_node_modules: options.include_node_modules,
            file_kind: ScanFileKind::Files,
            metadata: MetadataLevel::Size,
            order: ScanOrder::Path,
            max_files,
        },
        &control,
    )?;
    if result.completion != WorkCompletion::Complete {
        return Err(AstError::Limit(format!(
            "workspace selection exceeds the {max_files} file limit"
        )));
    }

    Ok(result.entries)
}

pub(crate) fn read_source(
    candidate: &WorkspaceEntry,
    max_bytes: usize,
    cancellation: &crate::workspace::Cancellation,
) -> Result<String, Issue> {
    let control = WorkControl::new(cancellation.clone(), None);
    let content = read_content(
        &candidate.path,
        ReadExtent::Complete {
            max_bytes: max_bytes as u64,
        },
        &control,
    )
    .map_err(|error| issue(candidate, "read_error", workspace_read_message(error)))?;
    if !content.complete {
        return Err(issue(
            candidate,
            "limit_reached",
            format!("file exceeds the {max_bytes} byte limit"),
        ));
    }
    if content.binary {
        return Err(issue(
            candidate,
            "read_error",
            "file contains a NUL byte".to_owned(),
        ));
    }
    String::from_utf8(content.bytes).map_err(|_| {
        issue(
            candidate,
            "read_error",
            "file is not valid UTF-8".to_owned(),
        )
    })
}

fn build_match<D>(
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
    (
        candidate.relative.clone(),
        canonical_name(language).to_owned(),
        source[byte_range.clone()].to_owned(),
        source_range(source, byte_range),
        captures,
    )
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

pub(crate) fn parse_issue(candidate: &WorkspaceEntry, language: SupportLang) -> Issue {
    (
        Some(candidate.relative.clone()),
        Some(canonical_name(language).to_owned()),
        "parse_error".to_owned(),
        "source contains syntax-tree error nodes".to_owned(),
    )
}

fn issue(candidate: &WorkspaceEntry, kind: &str, message: String) -> Issue {
    (
        Some(candidate.relative.clone()),
        None,
        kind.to_owned(),
        message,
    )
}

fn workspace_read_message(error: crate::workspace::WorkspaceError) -> String {
    match error {
        crate::workspace::WorkspaceError::Read(message) => message,
        crate::workspace::WorkspaceError::Cancelled => "AST operation cancelled".to_owned(),
        crate::workspace::WorkspaceError::Deadline => {
            "AST operation reached its deadline".to_owned()
        }
        other => format!("{other:?}"),
    }
}

fn limit_issue(max_matches: usize) -> Issue {
    (
        None,
        None,
        "limit_reached".to_owned(),
        format!("search stopped at the {max_matches} match limit"),
    )
}
