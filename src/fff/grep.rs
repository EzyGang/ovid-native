use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use fff_search::{
    Constraint, FFFQuery, FuzzyQuery, GitStatusFilter, GrepMode, GrepSearchOptions,
    has_regex_metacharacters,
};
use std::time::{Duration, Instant};

use crate::fff::FffError;
use crate::fff::engine::FffEngineState;
use crate::fff::find::{normalize_git_status, normalize_path};
use crate::fff::types::{
    FffConstraints, FffGrepRequest, FffMultiGrepRequest, NativeFffContextLine, NativeFffGrepMatch,
    NativeFffGrepResult,
};

pub(crate) fn grep(
    engine: &FffEngineState,
    request: FffGrepRequest,
    cancellation: Arc<AtomicBool>,
) -> Result<NativeFffGrepResult, FffError> {
    engine.ensure_started()?;
    validate_request_limits(
        engine,
        request.query.len(),
        request.limit,
        request.matches_per_file,
        request.context_before,
        request.context_after,
        request.timeout_seconds,
    )?;
    if request.query.is_empty() {
        return Err(FffError::Query("grep query must not be empty".to_owned()));
    }

    let query = build_query(&request.constraints, &request.query)?;
    let initial_mode = actual_mode(&request.mode, &request.query)?;
    let result = execute_grep(
        engine,
        &query,
        &request,
        initial_mode,
        cancellation.clone(),
        mode_name(initial_mode),
        None,
        initial_mode == GrepMode::Fuzzy,
    )?;
    if request.mode == "auto"
        && result.0.is_empty()
        && result.9.is_none()
        && initial_mode != GrepMode::Fuzzy
    {
        return execute_grep(
            engine,
            &query,
            &request,
            GrepMode::Fuzzy,
            cancellation,
            "fuzzy",
            Some(mode_name(initial_mode)),
            true,
        );
    }

    Ok(result)
}

pub(crate) fn multi_grep(
    engine: &FffEngineState,
    request: FffMultiGrepRequest,
    cancellation: Arc<AtomicBool>,
) -> Result<NativeFffGrepResult, FffError> {
    engine.ensure_started()?;
    let total_characters = request.patterns.iter().map(String::len).sum();
    validate_request_limits(
        engine,
        total_characters,
        request.limit,
        request.matches_per_file,
        request.context_before,
        request.context_after,
        request.timeout_seconds,
    )?;
    if request.patterns.is_empty() || request.patterns.iter().any(String::is_empty) {
        return Err(FffError::Query(
            "multi_grep patterns must be non-empty".to_owned(),
        ));
    }
    if request.patterns.len() > engine.limits.max_patterns {
        return Err(FffError::Limit(
            "multi_grep exceeds the pattern count limit".to_owned(),
        ));
    }
    if request
        .patterns
        .iter()
        .any(|pattern| pattern.len() > engine.limits.max_pattern_characters)
    {
        return Err(FffError::Limit(
            "multi_grep pattern exceeds the character limit".to_owned(),
        ));
    }

    let parsed = build_query(&request.constraints, "")?;
    let options = grep_options(
        engine,
        request.smart_case,
        request.file_offset,
        request.limit,
        request.matches_per_file,
        request.context_before,
        request.context_after,
        request.max_file_bytes,
        request.timeout_seconds,
        request.classify_definitions,
        GrepMode::PlainText,
        cancellation,
    )?;
    let patterns: Vec<&str> = request.patterns.iter().map(String::as_str).collect();
    let guard = engine
        .picker
        .read()
        .map_err(|error| FffError::Runtime(error.to_string()))?;
    let picker = guard
        .as_ref()
        .ok_or_else(|| FffError::Runtime("FFF picker is unavailable".to_owned()))?;
    let started = Instant::now();
    let result = picker.multi_grep(&patterns, &parsed.constraints, &options);

    map_grep_result(
        picker,
        result,
        "plain",
        None,
        false,
        true,
        time_budget_reached(started, request.timeout_seconds),
    )
}

#[allow(clippy::too_many_arguments)]
fn execute_grep(
    engine: &FffEngineState,
    query: &FFFQuery<'_>,
    request: &FffGrepRequest,
    mode: GrepMode,
    cancellation: Arc<AtomicBool>,
    actual_mode: &str,
    fallback_from: Option<&str>,
    approximate: bool,
) -> Result<NativeFffGrepResult, FffError> {
    let options = grep_options(
        engine,
        request.smart_case,
        request.file_offset,
        request.limit,
        request.matches_per_file,
        request.context_before,
        request.context_after,
        request.max_file_bytes,
        request.timeout_seconds,
        request.classify_definitions,
        mode,
        cancellation,
    )?;
    let guard = engine
        .picker
        .read()
        .map_err(|error| FffError::Runtime(error.to_string()))?;
    let picker = guard
        .as_ref()
        .ok_or_else(|| FffError::Runtime("FFF picker is unavailable".to_owned()))?;
    let started = Instant::now();
    let result = picker.grep(query, &options);
    if let Some(error) = result.regex_fallback_error.as_ref() {
        return Err(FffError::Pattern(error.clone()));
    }

    map_grep_result(
        picker,
        result,
        actual_mode,
        fallback_from,
        approximate,
        true,
        time_budget_reached(started, request.timeout_seconds),
    )
}

fn map_grep_result(
    picker: &fff_search::FilePicker,
    result: fff_search::GrepResult<'_>,
    actual_mode: &str,
    fallback_from: Option<&str>,
    approximate: bool,
    index_complete: bool,
    time_budget_reached: bool,
) -> Result<NativeFffGrepResult, FffError> {
    let matches = result
        .matches
        .iter()
        .map(|matched| {
            let file = result.files.get(matched.file_index).ok_or_else(|| {
                FffError::Runtime("FFF returned an invalid file index".to_owned())
            })?;
            let before = context_lines(matched.line_number, &matched.context_before, true);
            let after = context_lines(matched.line_number, &matched.context_after, false);
            Ok((
                normalize_path(file.relative_path(picker)),
                matched.line_number as usize,
                matched.col + 1,
                matched.byte_offset,
                matched.line_content.clone(),
                matched
                    .match_byte_offsets
                    .iter()
                    .map(|(start, end)| (*start as usize, *end as usize))
                    .collect(),
                before,
                after,
                approximate || matched.fuzzy_score.is_some(),
                matched.is_definition,
                normalize_git_status(fff_search::git::format_git_status(file.git_status)),
            ) as NativeFffGrepMatch)
        })
        .collect::<Result<Vec<_>, FffError>>()?;
    let completion = if !index_complete {
        "index_incomplete"
    } else if time_budget_reached && result.next_file_offset != 0 {
        "time_budget_reached"
    } else if result.next_file_offset == 0 {
        "complete"
    } else {
        "page_limit_reached"
    };
    let next_offset = (result.next_file_offset != 0).then_some(result.next_file_offset);

    Ok((
        matches,
        actual_mode.to_owned(),
        fallback_from.map(str::to_owned),
        approximate,
        completion.to_owned(),
        result.total_files,
        result.filtered_file_count,
        result.total_files_searched,
        result.files_with_matches,
        next_offset,
        index_complete,
    ))
}

fn time_budget_reached(started: Instant, timeout_seconds: f64) -> bool {
    started.elapsed() >= Duration::from_secs_f64(timeout_seconds)
}

fn context_lines(line_number: u64, lines: &[String], before: bool) -> Vec<NativeFffContextLine> {
    lines
        .iter()
        .enumerate()
        .map(|(index, text)| {
            let number = if before {
                line_number.saturating_sub(lines.len() as u64 - index as u64)
            } else {
                line_number + index as u64 + 1
            };
            (number as usize, text.clone())
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn grep_options(
    engine: &FffEngineState,
    smart_case: bool,
    file_offset: usize,
    limit: usize,
    matches_per_file: usize,
    context_before: usize,
    context_after: usize,
    max_file_bytes: u64,
    timeout_seconds: f64,
    classify_definitions: bool,
    mode: GrepMode,
    cancellation: Arc<AtomicBool>,
) -> Result<GrepSearchOptions, FffError> {
    if max_file_bytes > engine.limits.max_file_bytes {
        return Err(FffError::Limit(
            "grep exceeds the maximum file byte limit".to_owned(),
        ));
    }

    Ok(GrepSearchOptions {
        max_file_size: max_file_bytes,
        max_matches_per_file: matches_per_file,
        smart_case,
        file_offset,
        page_limit: limit,
        mode,
        time_budget_ms: (timeout_seconds * 1_000.0) as u64,
        before_context: context_before,
        after_context: context_after,
        classify_definitions,
        trim_whitespace: false,
        abort_signal: Some(cancellation),
    })
}

pub(crate) fn build_query<'a>(
    constraints: &'a FffConstraints,
    query: &'a str,
) -> Result<FFFQuery<'a>, FffError> {
    let mut parsed = Vec::with_capacity(constraints.include.len() + constraints.exclude.len() + 1);
    for value in &constraints.include {
        parsed.push(parse_path_constraint(value)?);
    }
    for value in &constraints.exclude {
        parsed.push(Constraint::Not(Box::new(parse_path_constraint(value)?)));
    }
    if let Some(status) = constraints.git_status.as_deref() {
        let status = match status {
            "clean" => GitStatusFilter::Unmodified,
            "modified" => GitStatusFilter::Modified,
            "staged" => GitStatusFilter::Staged,
            "untracked" => GitStatusFilter::Untracked,
            _ => {
                return Err(FffError::Query(
                    "unknown FFF Git status constraint".to_owned(),
                ));
            }
        };
        parsed.push(Constraint::GitStatus(status));
    }

    Ok(FFFQuery {
        raw_query: query,
        constraints: parsed,
        fuzzy_query: if query.is_empty() {
            FuzzyQuery::Empty
        } else {
            FuzzyQuery::Text(query)
        },
        location: None,
    })
}

fn parse_path_constraint(value: &str) -> Result<Constraint<'_>, FffError> {
    validate_constraint(value)?;
    if value.ends_with('/') {
        return Ok(Constraint::PathSegment(value.trim_end_matches('/')));
    }
    if fff_search::glob_detect::has_wildcards(value) {
        globset::Glob::new(value).map_err(|error| FffError::Query(error.to_string()))?;
        return Ok(Constraint::Glob(value));
    }

    Ok(Constraint::FilePath(value))
}

fn validate_constraint(value: &str) -> Result<(), FffError> {
    let has_parent = value.split(['/', '\\']).any(|component| component == "..");
    let has_drive_prefix = value.as_bytes().get(1) == Some(&b':');
    if value.is_empty()
        || value.starts_with(['/', '\\', '!'])
        || has_drive_prefix
        || has_parent
        || value
            .chars()
            .any(|character| character.is_ascii_whitespace() || character == '\0')
    {
        return Err(FffError::Query(
            "FFF constraints must be safe relative tokens".to_owned(),
        ));
    }

    Ok(())
}

pub(crate) fn validate_request_limits(
    engine: &FffEngineState,
    query_characters: usize,
    limit: usize,
    matches_per_file: usize,
    context_before: usize,
    context_after: usize,
    timeout_seconds: f64,
) -> Result<(), FffError> {
    if query_characters > engine.limits.max_query_characters
        || limit > engine.limits.max_results
        || matches_per_file > engine.limits.max_matches_per_file
        || limit.saturating_add(matches_per_file).saturating_sub(1) > engine.limits.max_results
        || context_before > engine.limits.max_context_lines
        || context_after > engine.limits.max_context_lines
        || (timeout_seconds > 0.0 && timeout_seconds > engine.limits.max_search_timeout_seconds)
    {
        return Err(FffError::Limit(
            "FFF request exceeds configured limits".to_owned(),
        ));
    }

    Ok(())
}

fn actual_mode(mode: &str, query: &str) -> Result<GrepMode, FffError> {
    match mode {
        "plain" => Ok(GrepMode::PlainText),
        "fuzzy" => Ok(GrepMode::Fuzzy),
        "regex" => {
            regex::Regex::new(query).map_err(|error| FffError::Pattern(error.to_string()))?;
            Ok(GrepMode::Regex)
        }
        "auto" if has_regex_metacharacters(query) => {
            regex::Regex::new(query).map_err(|error| FffError::Pattern(error.to_string()))?;
            Ok(GrepMode::Regex)
        }
        "auto" => Ok(GrepMode::PlainText),
        _ => Err(FffError::Configuration("unknown FFF grep mode".to_owned())),
    }
}

fn mode_name(mode: GrepMode) -> &'static str {
    match mode {
        GrepMode::PlainText => "plain",
        GrepMode::Regex => "regex",
        GrepMode::Fuzzy => "fuzzy",
    }
}

pub(crate) fn cancelled(signal: &AtomicBool) -> Result<(), FffError> {
    if signal.load(Ordering::Acquire) {
        return Err(FffError::Cancelled);
    }

    Ok(())
}
