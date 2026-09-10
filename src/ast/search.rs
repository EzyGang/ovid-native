use std::collections::HashMap;

use ast_grep_core::Pattern;

use crate::ast::{
    AstError,
    language::{resolve_language, strictness},
    matches::build_match,
    source::{classify_files, discover_files, read_tree},
    types::{Issue, SearchRequest, SearchResult},
};

pub fn search(root: &std::path::Path, request: SearchRequest) -> Result<SearchResult, AstError> {
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
        root,
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
        let Some(tree) = read_tree(
            &candidate,
            language,
            request.limits.max_file_bytes,
            &request.cancellation,
            &mut issues,
        )?
        else {
            continue;
        };
        let source = tree.source();
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
                    source,
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

fn limit_issue(max_matches: usize) -> Issue {
    (
        None,
        None,
        "limit_reached".to_owned(),
        format!("search stopped at the {max_matches} match limit"),
    )
}
