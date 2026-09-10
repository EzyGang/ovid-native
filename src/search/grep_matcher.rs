use std::{io, ops::Range};

use grep_matcher::Matcher;
use grep_searcher::{BinaryDetection, Searcher, SearcherBuilder, Sink, SinkMatch};

use crate::search::{SearchError, types::GrepRequest};
use crate::workspace::{WorkControl, WorkStopped, WorkspaceError};

pub(crate) enum CompiledMatcher {
    Rust(grep_regex::RegexMatcher),
    Pcre2(grep_pcre2::RegexMatcher),
}

pub(crate) struct CompiledPattern {
    pub(crate) matcher: CompiledMatcher,
    pub(crate) engine: &'static str,
    pub(crate) interpreted_as_literal: bool,
}

pub(crate) struct CollectedMatches {
    pub(crate) ranges: Vec<Range<usize>>,
    pub(crate) total_matches: usize,
    pub(crate) total_matches_exact: bool,
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

pub(crate) fn collect_matches<M>(
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

pub(crate) fn compile_pattern(request: &GrepRequest) -> Result<CompiledPattern, SearchError> {
    let literal = request.mode == "literal";
    let rust_error = match rust_matcher(
        &request.pattern,
        request.case_sensitive,
        request.multiline,
        literal,
    ) {
        Ok(matcher) => {
            return Ok(CompiledPattern {
                matcher: CompiledMatcher::Rust(matcher),
                engine: "rust",
                interpreted_as_literal: false,
            });
        }
        Err(error) => error,
    };

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

    Err(SearchError::Pattern(rust_error.to_string()))
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
