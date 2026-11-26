use fff_search::{FuzzySearchOptions, MixedItemRef, PaginationArgs, Score};

use crate::fff::FffError;
use crate::fff::engine::FffEngineState;
use crate::fff::query::{build_query, validate_request_limits};
use crate::fff::types::{FffFindRequest, NativeFffFindResult, NativeFffPathMatch};

pub(crate) fn find(
    engine: &FffEngineState,
    request: FffFindRequest,
) -> Result<NativeFffFindResult, FffError> {
    engine.ensure_started()?;
    validate_request_limits(engine, request.query.len(), request.limit, 1, 0, 0, 0.0)?;
    if !request.query.chars().any(char::is_alphanumeric)
        && request.constraints.include.is_empty()
        && request.constraints.exclude.is_empty()
        && request.constraints.git_status.is_none()
    {
        return Err(FffError::Query(
            "find query must contain an alphanumeric character or typed constraints".to_owned(),
        ));
    }

    let query = build_query(&request.constraints, &request.query)?;
    let options = FuzzySearchOptions {
        pagination: PaginationArgs {
            offset: request.offset,
            limit: request.limit,
        },
        ..Default::default()
    };
    let (matches, total_matches) = engine.with_picker(|picker| match request.kind.as_str() {
        "file" => {
            let result = picker.fuzzy_search(&query, None, options);
            let values = path_matches(picker, result.items, result.scores, file_match);
            Ok((values, result.total_matched))
        }
        "directory" => {
            let result = picker.fuzzy_search_directories(&query, options);
            let values = path_matches(picker, result.items, result.scores, directory_match);
            Ok((values, result.total_matched))
        }
        "any" => {
            let result = picker.fuzzy_search_mixed(&query, None, options);
            let values = path_matches(picker, result.items, result.scores, mixed_match);
            Ok((values, result.total_matched))
        }
        _ => Err(FffError::Configuration("unknown FFF find kind".to_owned())),
    })?;
    let next_offset =
        (request.offset + matches.len() < total_matches).then_some(request.offset + matches.len());

    Ok((matches, total_matches, next_offset, true))
}

fn path_matches<I, F>(
    picker: &fff_search::FilePicker,
    items: Vec<I>,
    scores: Vec<Score>,
    path_match: F,
) -> Vec<NativeFffPathMatch>
where
    F: Fn(&fff_search::FilePicker, I, bool) -> NativeFffPathMatch,
{
    items
        .into_iter()
        .zip(scores)
        .map(|(item, score)| path_match(picker, item, score.exact_match))
        .collect()
}

fn file_match(
    picker: &fff_search::FilePicker,
    item: &fff_search::FileItem,
    exact_match: bool,
) -> NativeFffPathMatch {
    let status = normalize_git_status(fff_search::git::format_git_status(item.git_status));
    (
        normalize_path(item.relative_path(picker)),
        "file".to_owned(),
        exact_match,
        Some(item.size),
        Some(item.modified),
        status,
    )
}

fn directory_match(
    picker: &fff_search::FilePicker,
    item: &fff_search::DirItem,
    exact_match: bool,
) -> NativeFffPathMatch {
    let mut path = normalize_path(item.relative_path(picker));
    if !path.ends_with('/') {
        path.push('/');
    }

    (
        path,
        "directory".to_owned(),
        exact_match,
        None,
        None,
        "unknown".to_owned(),
    )
}

fn mixed_match(
    picker: &fff_search::FilePicker,
    item: MixedItemRef<'_>,
    exact_match: bool,
) -> NativeFffPathMatch {
    match item {
        MixedItemRef::File(item) => file_match(picker, item, exact_match),
        MixedItemRef::Dir(item) => directory_match(picker, item, exact_match),
    }
}

pub(crate) fn normalize_path(value: String) -> String {
    value.replace('\\', "/")
}

pub(crate) fn normalize_git_status(value: &str) -> String {
    match value {
        "staged_new" | "staged_modified" | "staged_deleted" => "staged".to_owned(),
        other => other.to_owned(),
    }
}
