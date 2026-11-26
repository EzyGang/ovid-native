use std::collections::HashMap;

use ast_grep_core::Pattern;

use crate::ast::{
    AstError,
    matches::build_match,
    source::{Selection, select},
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
    let Selection {
        files,
        unsupported_files,
        strictness,
        mut reader,
    } = select(root, &request.scope)?;
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
    let mut total_matches = 0;
    let mut files_with_matches = 0;
    let mut limit_reached = false;

    'files: for (candidate, language) in files {
        let Some(tree) = reader.read(&candidate, language)? else {
            continue;
        };
        let source = tree.source();
        let before_file = total_matches;
        let pattern = &patterns[&language];
        for matched in tree.root().find_all(pattern) {
            if request.scope.cancellation.is_cancelled() {
                return Err(AstError::Cancelled);
            }
            if total_matches == request.scope.limits.max_matches {
                limit_reached = true;
                reader
                    .issues
                    .push(limit_issue(request.scope.limits.max_matches));
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
        reader.files_searched,
        files_with_matches,
        unsupported_files,
        truncated,
        reader.issues,
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
