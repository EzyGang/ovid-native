from datetime import UTC, datetime
from typing import cast

from ovid_native import _native
from ovid_native.search.models import (
    GlobFileType,
    GlobMatch,
    GlobResult,
    GrepContextLine,
    GrepFileCoverage,
    GrepFileMatches,
    GrepMatch,
    GrepPosition,
    GrepRange,
    GrepRegexEngine,
    GrepResult,
    SearchCompletion,
)


def glob_result(value: _native.NativeGlobResult) -> GlobResult:
    matches, completion, scanned_entries, skipped_entries, truncated = value
    return GlobResult(
        matches=tuple(_glob_match(match) for match in matches),
        completion=cast('SearchCompletion', completion),
        scanned_entries=scanned_entries,
        skipped_entries=skipped_entries,
        truncated=truncated,
    )


def grep_result(value: _native.NativeGrepResult) -> GrepResult:
    (
        files,
        pattern_engine,
        interpreted_as_literal,
        completion,
        files_searched,
        files_with_matches,
        files_with_matches_exact,
        skipped_binary_files,
        skipped_encoding_files,
        skipped_large_files,
        next_file_offset,
        truncated,
    ) = value
    
    return GrepResult(
        files=tuple(_grep_file(file) for file in files),
        pattern_engine=cast('GrepRegexEngine', pattern_engine),
        interpreted_as_literal=interpreted_as_literal,
        completion=cast('SearchCompletion', completion),
        files_searched=files_searched,
        files_with_matches=files_with_matches,
        files_with_matches_exact=files_with_matches_exact,
        skipped_binary_files=skipped_binary_files,
        skipped_encoding_files=skipped_encoding_files,
        skipped_large_files=skipped_large_files,
        next_file_offset=next_file_offset,
        truncated=truncated,
    )


def _glob_match(value: _native.NativeGlobMatch) -> GlobMatch:
    path, file_type, size, modified_at = value
    
    return GlobMatch(
        path=path,
        file_type=cast('GlobFileType', file_type),
        size=size,
        modified_at=None if modified_at is None else datetime.fromtimestamp(modified_at, tz=UTC),
    )


def _grep_file(value: _native.NativeGrepFileMatches) -> GrepFileMatches:
    path, matches, total_matches, matches_truncated, total_matches_exact, coverage = value
    searched_bytes, total_bytes, complete = coverage
    
    return GrepFileMatches(
        path=path,
        matches=tuple(_grep_match(match) for match in matches),
        total_matches=total_matches,
        matches_truncated=matches_truncated,
        total_matches_exact=total_matches_exact,
        coverage=GrepFileCoverage(searched_bytes=searched_bytes, total_bytes=total_bytes, complete=complete),
    )


def _grep_match(value: _native.NativeGrepMatch) -> GrepMatch:
    text, match_range, line_text, line_truncated, context_before, context_after = value
    
    return GrepMatch(
        text=text,
        range=_grep_range(match_range),
        line_text=line_text,
        line_truncated=line_truncated,
        context_before=tuple(_context_line(line) for line in context_before),
        context_after=tuple(_context_line(line) for line in context_after),
    )


def _grep_range(value: _native.NativeGrepRange) -> GrepRange:
    start, end = value
    return GrepRange(start=_position(start), end=_position(end))


def _position(value: _native.NativeGrepPosition) -> GrepPosition:
    line, column, byte_offset = value
    return GrepPosition(line=line, column=column, byte_offset=byte_offset)


def _context_line(value: _native.NativeGrepContextLine) -> GrepContextLine:
    line_number, text, truncated = value
    return GrepContextLine(line_number=line_number, text=text, truncated=truncated)
