use fff_search::{Constraint, FFFQuery, FuzzyQuery, GitStatusFilter};

use crate::fff::{FffError, engine::FffEngineState, types::FffConstraints};

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
