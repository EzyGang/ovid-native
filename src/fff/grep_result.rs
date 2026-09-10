use std::time::{Duration, Instant};

use crate::fff::{
    FffError,
    find::{normalize_git_status, normalize_path},
    types::{NativeFffContextLine, NativeFffGrepMatch, NativeFffGrepResult},
};

pub(crate) fn map_grep_result(
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

pub(crate) fn time_budget_reached(started: Instant, timeout_seconds: f64) -> bool {
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
