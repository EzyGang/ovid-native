use std::ops::Range;

use crate::search::{
    grep_matcher::{CompiledMatcher, CompiledPattern, collect_matches},
    types::{
        GrepRequest, NativeGrepContextLine, NativeGrepFileMatches, NativeGrepMatch,
        NativeGrepPosition, NativeGrepRange,
    },
};
use crate::workspace::{ReadExtent, WorkControl, WorkspaceEntry, WorkspaceError, read_content};

pub(crate) enum FileSearch {
    Binary,
    Encoding,
    Searched(NativeGrepFileMatches),
}

pub(crate) fn search_file(
    entry: &WorkspaceEntry,
    request: &GrepRequest,
    compiled: &CompiledPattern,
    control: &WorkControl,
) -> Result<FileSearch, WorkspaceError> {
    let extent = ReadExtent::Prefix {
        max_bytes: request.max_file_bytes as u64,
    };
    let content = read_content(&entry.path, extent, control)?;
    if content.binary {
        return Ok(FileSearch::Binary);
    }
    let (source, searched_bytes) = match std::str::from_utf8(&content.bytes) {
        Ok(source) => (source, content.searched_bytes),
        Err(error) if !content.complete && error.error_len().is_none() => {
            let valid_bytes = &content.bytes[..error.valid_up_to()];
            let source = std::str::from_utf8(valid_bytes)
                .map_err(|_| WorkspaceError::Read("invalid UTF-8 prefix boundary".to_owned()))?;
            (source, valid_bytes.len() as u64)
        }
        Err(_) => return Ok(FileSearch::Encoding),
    };
    let line_starts = line_starts(source);
    let collected = match &compiled.matcher {
        CompiledMatcher::Rust(matcher) => {
            collect_matches(matcher, source.as_bytes(), request, control)?
        }
        CompiledMatcher::Pcre2(matcher) => {
            collect_matches(matcher, source.as_bytes(), request, control)?
        }
    };
    let retained = collected
        .ranges
        .iter()
        .take(request.matches_per_file)
        .map(|range| build_match(source, &line_starts, range.clone(), request))
        .collect::<Vec<_>>();
    let matches_truncated =
        retained.len() < collected.total_matches || !collected.total_matches_exact;

    Ok(FileSearch::Searched((
        entry.relative.clone(),
        retained,
        collected.total_matches,
        matches_truncated,
        collected.total_matches_exact,
        (searched_bytes, content.total_bytes, content.complete),
    )))
}

fn build_match(
    source: &str,
    line_starts: &[usize],
    range: Range<usize>,
    request: &GrepRequest,
) -> NativeGrepMatch {
    let start_line = line_index(line_starts, range.start);
    let matched_end = if range.end > range.start {
        range.end - 1
    } else {
        range.start
    };
    let end_line = line_index(line_starts, matched_end);
    let (line_text, line_truncated) =
        display_line(source, line_starts, start_line, request.max_line_characters);
    let matched_lines = context_lines(
        source,
        line_starts,
        start_line..end_line + 1,
        request.max_line_characters,
    );
    let context_before = context_lines(
        source,
        line_starts,
        start_line.saturating_sub(request.context_before)..start_line,
        request.max_line_characters,
    );
    let context_after = context_lines(
        source,
        line_starts,
        (end_line + 1)..(end_line + 1 + request.context_after).min(line_starts.len()),
        request.max_line_characters,
    );

    (
        source[range.clone()].to_owned(),
        (
            position(source, line_starts, range.start),
            position(source, line_starts, range.end),
        ) as NativeGrepRange,
        line_text,
        line_truncated,
        matched_lines,
        context_before,
        context_after,
    )
}

fn line_starts(source: &str) -> Vec<usize> {
    let mut starts = vec![0];
    starts.extend(
        source
            .bytes()
            .enumerate()
            .filter_map(|(index, byte)| (byte == b'\n').then_some(index + 1)),
    );
    starts
}

fn line_index(starts: &[usize], offset: usize) -> usize {
    starts
        .partition_point(|start| *start <= offset)
        .saturating_sub(1)
}

fn position(source: &str, starts: &[usize], offset: usize) -> NativeGrepPosition {
    let index = line_index(starts, offset);
    let column = source[starts[index]..offset].chars().count() + 1;
    (index + 1, column, offset)
}

fn context_lines(
    source: &str,
    starts: &[usize],
    range: Range<usize>,
    max_characters: usize,
) -> Vec<NativeGrepContextLine> {
    range
        .filter(|index| *index < starts.len())
        .map(|index| {
            let (text, truncated) = display_line(source, starts, index, max_characters);
            (index + 1, text, truncated)
        })
        .collect()
}

fn display_line(
    source: &str,
    starts: &[usize],
    index: usize,
    max_characters: usize,
) -> (String, bool) {
    let start = starts[index];
    let end = starts
        .get(index + 1)
        .copied()
        .unwrap_or(source.len())
        .saturating_sub(usize::from(starts.get(index + 1).is_some()));
    let line = source[start..end]
        .strip_suffix('\r')
        .unwrap_or(&source[start..end]);
    let mut characters = line.chars();
    let text = characters.by_ref().take(max_characters).collect::<String>();
    let truncated = characters.next().is_some();
    (text, truncated)
}
