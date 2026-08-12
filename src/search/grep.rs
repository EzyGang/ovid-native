use std::io;
use std::ops::Range;
use std::time::Duration;

use grep_matcher::Matcher;
use grep_searcher::{BinaryDetection, Searcher, SearcherBuilder, Sink, SinkMatch};

use crate::search::SearchError;
use crate::search::glob::completion;
use crate::search::types::{
    GrepRequest, NativeGrepContextLine, NativeGrepFileMatches, NativeGrepMatch, NativeGrepPosition,
    NativeGrepRange, NativeGrepResult,
};
use crate::workspace::{
    MetadataLevel, ReadExtent, ScanFileKind, ScanOrder, ScanRequest, WorkCompletion, WorkControl,
    WorkStopped, Workspace, WorkspaceEntry, WorkspaceError, read_content,
};

enum CompiledMatcher {
    Rust(grep_regex::RegexMatcher),
    Pcre2(grep_pcre2::RegexMatcher),
}

struct CompiledPattern {
    matcher: CompiledMatcher,
    engine: &'static str,
    interpreted_as_literal: bool,
}

struct CollectedMatches {
    ranges: Vec<Range<usize>>,
    total_matches: usize,
    total_matches_exact: bool,
}

enum FileSearch {
    Binary,
    Encoding,
    Searched(NativeGrepFileMatches),
}

struct MatchSink<'m, M> {
    matcher: &'m M,
    control: &'m WorkControl,
    ranges: Vec<Range<usize>>,
    safety_limit: usize,
    limit_reached: bool,
    stopped: Option<WorkStopped>,
}

impl<M> Sink for MatchSink<'_, M>
where
    M: Matcher,
{
    type Error = io::Error;

    fn matched(
        &mut self,
        _searcher: &Searcher,
        matched_lines: &SinkMatch<'_>,
    ) -> Result<bool, Self::Error> {
        if let Err(stopped) = self.control.checkpoint() {
            self.stopped = Some(stopped);
            return Ok(false);
        }
        let absolute = usize::try_from(matched_lines.absolute_byte_offset())
            .map_err(|_| io::Error::other("match offset exceeds platform limits"))?;
        self.matcher
            .find_iter(matched_lines.bytes(), |matched| {
                if let Err(stopped) = self.control.checkpoint() {
                    self.stopped = Some(stopped);
                    return false;
                }
                if self.ranges.len() > self.safety_limit {
                    self.limit_reached = true;
                    return false;
                }
                self.ranges
                    .push(absolute + matched.start()..absolute + matched.end());
                true
            })
            .map_err(|error| io::Error::other(error.to_string()))?;

        Ok(!self.limit_reached)
    }
}

pub(crate) fn grep(
    workspace: &Workspace,
    request: GrepRequest,
) -> Result<NativeGrepResult, SearchError> {
    validate_request(&request)?;
    let compiled = compile_pattern(&request)?;
    let control = WorkControl::new(
        request.cancellation.clone(),
        Some(Duration::from_secs_f64(request.timeout_seconds)),
    );
    let scan = workspace.scan(
        &ScanRequest {
            selections: request.paths.clone(),
            include_hidden: request.include_hidden,
            respect_gitignore: request.respect_gitignore,
            include_node_modules: request.include_node_modules,
            file_kind: ScanFileKind::Files,
            metadata: MetadataLevel::Size,
            order: ScanOrder::Path,
            max_files: request.max_scan_files,
        },
        &control,
    )?;

    search_entries(scan, &request, &compiled, &control)
}

fn search_entries(
    scan: crate::workspace::ScanResult,
    request: &GrepRequest,
    compiled: &CompiledPattern,
    control: &WorkControl,
) -> Result<NativeGrepResult, SearchError> {
    let mut files = Vec::new();
    let mut files_searched = 0;
    let mut files_with_matches = 0;
    let mut skipped_binary_files = 0;
    let mut skipped_encoding_files = 0;
    let mut skipped_large_files = 0;
    let mut retained_matches = 0;
    let mut found_extra_file = false;
    let mut stopped_early = false;
    let mut operation_completion = scan.completion;

    for entry in scan.entries {
        match control.checkpoint() {
            Ok(()) => (),
            Err(WorkStopped::Cancelled) => return Err(SearchError::Cancelled),
            Err(WorkStopped::Deadline) => {
                operation_completion = WorkCompletion::DeadlineReached;
                stopped_early = true;
                break;
            }
        }
        if entry
            .size
            .is_some_and(|size| size > request.max_file_bytes as u64)
            && request.large_file_mode == "skip"
        {
            skipped_large_files += 1;
            continue;
        }

        let searched = match search_file(&entry, request, compiled, control) {
            Ok(value) => value,
            Err(WorkspaceError::Cancelled) => return Err(SearchError::Cancelled),
            Err(WorkspaceError::Deadline) => {
                operation_completion = WorkCompletion::DeadlineReached;
                stopped_early = true;
                break;
            }
            Err(WorkspaceError::Read(message)) => {
                return Err(SearchError::Read(format!(
                    "cannot read {}: {message}",
                    entry.relative
                )));
            }
            Err(error) => return Err(error.into()),
        };
        let mut file = match searched {
            FileSearch::Binary => {
                skipped_binary_files += 1;
                continue;
            }
            FileSearch::Encoding => {
                skipped_encoding_files += 1;
                continue;
            }
            FileSearch::Searched(file) => file,
        };
        files_searched += 1;
        if file.1.is_empty() && file.2 == 0 {
            continue;
        }

        files_with_matches += 1;
        if files_with_matches <= request.file_offset {
            continue;
        }
        if files.len() == request.file_limit {
            found_extra_file = true;
            stopped_early = true;
            break;
        }

        let available = request.max_grep_matches.saturating_sub(retained_matches);
        if file.1.len() > available {
            file.1.truncate(available);
            file.3 = true;
            stopped_early = true;
        }
        retained_matches += file.1.len();
        files.push(file);
        if retained_matches == request.max_grep_matches {
            stopped_early = true;
            break;
        }
    }

    let complete_counts = operation_completion == WorkCompletion::Complete && !stopped_early;
    let truncated = found_extra_file
        || stopped_early
        || operation_completion != WorkCompletion::Complete
        || files.iter().any(|file| file.3 || !file.4 || !file.5.2);
    let next_file_offset = if found_extra_file
        || operation_completion != WorkCompletion::Complete
        || (stopped_early && !complete_counts)
    {
        Some(request.file_offset + files.len())
    } else {
        None
    };

    Ok((
        files,
        compiled.engine.to_owned(),
        compiled.interpreted_as_literal,
        completion(operation_completion).to_owned(),
        files_searched,
        files_with_matches,
        complete_counts,
        skipped_binary_files,
        skipped_encoding_files,
        skipped_large_files,
        next_file_offset,
        truncated,
    ))
}

fn search_file(
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

fn collect_matches<M>(
    matcher: &M,
    source: &[u8],
    request: &GrepRequest,
    control: &WorkControl,
) -> Result<CollectedMatches, WorkspaceError>
where
    M: Matcher,
{
    let mut searcher = SearcherBuilder::new()
        .line_number(true)
        .multi_line(request.multiline)
        .before_context(request.context_before)
        .after_context(request.context_after)
        .binary_detection(BinaryDetection::quit(b'\0'))
        .bom_sniffing(false)
        .build();
    let mut sink = MatchSink {
        matcher,
        control,
        ranges: Vec::new(),
        safety_limit: request.max_matches_per_file,
        limit_reached: false,
        stopped: None,
    };
    searcher
        .search_slice(matcher, source, &mut sink)
        .map_err(|error| WorkspaceError::Read(format!("search failed: {error}")))?;
    if let Some(stopped) = sink.stopped {
        return Err(match stopped {
            WorkStopped::Cancelled => WorkspaceError::Cancelled,
            WorkStopped::Deadline => WorkspaceError::Deadline,
        });
    }
    sink.ranges.sort_by_key(|range| (range.start, range.end));
    sink.ranges.dedup();
    let exact = sink.ranges.len() <= request.max_matches_per_file && !sink.limit_reached;
    if sink.ranges.len() > request.max_matches_per_file {
        sink.ranges.truncate(request.max_matches_per_file);
    }
    let total_matches = sink.ranges.len();

    Ok(CollectedMatches {
        ranges: sink.ranges,
        total_matches,
        total_matches_exact: exact,
    })
}

fn compile_pattern(request: &GrepRequest) -> Result<CompiledPattern, SearchError> {
    let literal = request.mode == "literal";
    let rust = rust_matcher(
        &request.pattern,
        request.case_sensitive,
        request.multiline,
        literal,
    );
    if let Ok(matcher) = rust {
        return Ok(CompiledPattern {
            matcher: CompiledMatcher::Rust(matcher),
            engine: "rust",
            interpreted_as_literal: false,
        });
    }

    if request.mode != "literal"
        && let Ok(matcher) =
            pcre2_matcher(&request.pattern, request.case_sensitive, request.multiline)
    {
        return Ok(CompiledPattern {
            matcher: CompiledMatcher::Pcre2(matcher),
            engine: "pcre2",
            interpreted_as_literal: false,
        });
    }
    if request.mode == "auto" {
        let matcher = rust_matcher(
            &request.pattern,
            request.case_sensitive,
            request.multiline,
            true,
        )
        .map_err(|error| SearchError::Pattern(error.to_string()))?;
        return Ok(CompiledPattern {
            matcher: CompiledMatcher::Rust(matcher),
            engine: "rust",
            interpreted_as_literal: true,
        });
    }

    let rust_error = rust_matcher(
        &request.pattern,
        request.case_sensitive,
        request.multiline,
        literal,
    )
    .err()
    .map_or_else(
        || "unknown pattern error".to_owned(),
        |error| error.to_string(),
    );
    Err(SearchError::Pattern(rust_error))
}

fn rust_matcher(
    pattern: &str,
    case_sensitive: bool,
    multiline: bool,
    literal: bool,
) -> Result<grep_regex::RegexMatcher, grep_regex::Error> {
    let mut builder = grep_regex::RegexMatcherBuilder::new();
    builder
        .case_insensitive(!case_sensitive)
        .multi_line(true)
        .dot_matches_new_line(multiline)
        .fixed_strings(literal);
    if !multiline {
        builder.line_terminator(Some(b'\n'));
    }
    builder.build(pattern)
}

fn pcre2_matcher(
    pattern: &str,
    case_sensitive: bool,
    multiline: bool,
) -> Result<grep_pcre2::RegexMatcher, grep_pcre2::Error> {
    let mut builder = grep_pcre2::RegexMatcherBuilder::new();
    builder
        .caseless(!case_sensitive)
        .multi_line(true)
        .dotall(multiline)
        .utf(true)
        .ucp(true);
    builder.build(pattern)
}

fn validate_request(request: &GrepRequest) -> Result<(), SearchError> {
    if request.pattern.contains('\0') {
        return Err(SearchError::Pattern(
            "grep pattern must contain no NUL bytes".to_owned(),
        ));
    }
    if !matches!(request.mode.as_str(), "regex" | "literal" | "auto") {
        return Err(SearchError::Configuration(format!(
            "invalid grep pattern mode: {}",
            request.mode
        )));
    }
    if !matches!(request.large_file_mode.as_str(), "skip" | "prefix") {
        return Err(SearchError::Configuration(format!(
            "invalid large-file mode: {}",
            request.large_file_mode
        )));
    }

    Ok(())
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
