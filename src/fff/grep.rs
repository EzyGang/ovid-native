use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use fff_search::{FFFQuery, GrepMode, GrepSearchOptions, has_regex_metacharacters};

use crate::fff::{
    FffError,
    engine::FffEngineState,
    grep_result::{map_grep_result, time_budget_reached},
    query::{build_query, validate_request_limits},
    types::{FffGrepRequest, FffMultiGrepRequest, NativeFffGrepResult},
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

fn actual_mode(mode: &str, query: &str) -> Result<GrepMode, FffError> {
    let mode = match mode {
        "plain" => GrepMode::PlainText,
        "fuzzy" => GrepMode::Fuzzy,
        "regex" => GrepMode::Regex,
        "auto" if has_regex_metacharacters(query) => GrepMode::Regex,
        "auto" => GrepMode::PlainText,
        _ => return Err(FffError::Configuration("unknown FFF grep mode".to_owned())),
    };
    if mode == GrepMode::Regex {
        regex::Regex::new(query).map_err(|error| FffError::Pattern(error.to_string()))?;
    }
    Ok(mode)
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
