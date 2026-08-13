from datetime import UTC, datetime
from typing import Literal, cast

from ovid_native import _native
from ovid_native.fff.models import (
    FffActualGrepMode,
    FffByteRange,
    FffContextLine,
    FffFindResult,
    FffGitStatusValue,
    FffGrepMatch,
    FffGrepResult,
    FffIndexState,
    FffIndexStatus,
    FffPathKind,
    FffPathMatch,
    FffSearchCompletion,
)


def index_status(value: _native.NativeFffIndexStatus) -> FffIndexStatus:
    state, indexed_files, scan_complete, watch_enabled, content_index_enabled = value
    return FffIndexStatus(
        state=cast(FffIndexState, state),
        indexed_files=indexed_files,
        scan_complete=scan_complete,
        watch_enabled=watch_enabled,
        content_index_enabled=content_index_enabled,
    )


def find_result(value: _native.NativeFffFindResult) -> FffFindResult:
    matches, total_matches, next_offset, index_complete = value
    return FffFindResult(
        matches=tuple(_path_match(item) for item in matches),
        total_matches=total_matches,
        next_offset=next_offset,
        index_complete=index_complete,
    )


def grep_result(value: _native.NativeFffGrepResult) -> FffGrepResult:
    (
        matches,
        actual_mode,
        fallback_from,
        approximate,
        completion,
        indexed_files,
        searchable_files,
        files_searched,
        files_with_matches,
        next_file_offset,
        index_complete,
    ) = value
    return FffGrepResult(
        matches=tuple(_grep_match(item) for item in matches),
        actual_mode=cast(FffActualGrepMode, actual_mode),
        fallback_from=cast(Literal['plain', 'regex'] | None, fallback_from),
        approximate=approximate,
        completion=cast(FffSearchCompletion, completion),
        indexed_files=indexed_files,
        searchable_files=searchable_files,
        files_searched=files_searched,
        files_with_matches=files_with_matches,
        next_file_offset=next_file_offset,
        index_complete=index_complete,
    )


def _path_match(value: _native.NativeFffPathMatch) -> FffPathMatch:
    path, kind, exact_match, size, modified_at, git_status = value
    return FffPathMatch(
        path=path,
        kind=cast(FffPathKind, kind),
        exact_match=exact_match,
        size=size,
        modified_at=datetime.fromtimestamp(modified_at, tz=UTC) if modified_at is not None else None,
        git_status=cast(FffGitStatusValue, git_status),
    )


def _grep_match(value: _native.NativeFffGrepMatch) -> FffGrepMatch:
    path, line_number, column, byte_offset, line, ranges, before, after, approximate, definition, status = value
    return FffGrepMatch(
        path=path,
        line_number=line_number,
        column=column,
        byte_offset=byte_offset,
        line=line,
        match_ranges=tuple(FffByteRange(start=start, end=end) for start, end in ranges),
        context_before=tuple(FffContextLine(line_number=number, text=text) for number, text in before),
        context_after=tuple(FffContextLine(line_number=number, text=text) for number, text in after),
        approximate=approximate,
        is_definition=definition,
        git_status=cast(FffGitStatusValue, status),
    )
