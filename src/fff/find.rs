use fff_search::{FuzzySearchOptions, MixedItemRef, PaginationArgs};

use crate::fff::FffError;
use crate::fff::engine::FffEngineState;
use crate::fff::grep::{build_query, validate_request_limits};
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
    let guard = engine
        .picker
        .read()
        .map_err(|error| FffError::Runtime(error.to_string()))?;
    let picker = guard
        .as_ref()
        .ok_or_else(|| FffError::Runtime("FFF picker is unavailable".to_owned()))?;

    let (matches, total_matches) = match request.kind.as_str() {
        "file" => {
            let result = picker.fuzzy_search(&query, None, options);
            let values: Vec<NativeFffPathMatch> = result
                .items
                .into_iter()
                .zip(result.scores)
                .map(|(item, score)| file_match(picker, item, score.exact_match))
                .collect();
            (values, result.total_matched)
        }
        "directory" => {
            let result = picker.fuzzy_search_directories(&query, options);
            let values: Vec<NativeFffPathMatch> = result
                .items
                .into_iter()
                .zip(result.scores)
                .map(|(item, score)| directory_match(picker, item, score.exact_match))
                .collect();
            (values, result.total_matched)
        }
        "any" => {
            let result = picker.fuzzy_search_mixed(&query, None, options);
            let values: Vec<NativeFffPathMatch> = result
                .items
                .into_iter()
                .zip(result.scores)
                .map(|(item, score)| match item {
                    MixedItemRef::File(item) => file_match(picker, item, score.exact_match),
                    MixedItemRef::Dir(item) => directory_match(picker, item, score.exact_match),
                })
                .collect();
            (values, result.total_matched)
        }
        _ => return Err(FffError::Configuration("unknown FFF find kind".to_owned())),
    };
    let next_offset =
        (request.offset + matches.len() < total_matches).then_some(request.offset + matches.len());

    Ok((matches, total_matches, next_offset, true))
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

pub(crate) fn normalize_path(value: String) -> String {
    value.replace('\\', "/")
}

pub(crate) fn normalize_git_status(value: &str) -> String {
    match value {
        "staged_new" | "staged_modified" | "staged_deleted" => "staged".to_owned(),
        other => other.to_owned(),
    }
}
