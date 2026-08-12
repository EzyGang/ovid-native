from typing import cast

from ovid_native import _native
from ovid_native.ast.models import (
    AstCapture,
    AstChange,
    AstFileChange,
    AstIssue,
    AstIssueKind,
    AstLanguageInfo,
    AstMatch,
    AstPosition,
    AstRange,
    AstSearchResult,
)


def language_info(value: _native.NativeAstLanguageInfo) -> AstLanguageInfo:
    identifier, aliases, extensions = value
    return AstLanguageInfo(identifier=identifier, aliases=tuple(aliases), extensions=tuple(extensions))


def search_result(value: _native.NativeAstSearchResult) -> AstSearchResult:
    matches, total_matches, files_searched, files_with_matches, unsupported_files, truncated, issues = value
    return AstSearchResult(
        matches=tuple(_match(item) for item in matches),
        total_matches=total_matches,
        files_searched=files_searched,
        files_with_matches=files_with_matches,
        unsupported_files=unsupported_files,
        truncated=truncated,
        issues=tuple(issue(item) for item in issues),
    )


def changes(values: list[_native.NativeAstChange]) -> tuple[AstChange, ...]:
    return tuple(_change(value) for value in values)


def file_changes(values: list[_native.NativeAstFileChange]) -> tuple[AstFileChange, ...]:
    return tuple(_file_change(value) for value in values)


def issues(values: list[_native.NativeAstIssue]) -> tuple[AstIssue, ...]:
    return tuple(issue(value) for value in values)


def issue(value: _native.NativeAstIssue) -> AstIssue:
    path, language, kind, message = value
    return AstIssue(path=path, language=language, kind=cast('AstIssueKind', kind), message=message)


def _position(value: _native.NativeAstPosition) -> AstPosition:
    line, column, byte_offset = value
    return AstPosition(line=line, column=column, byte_offset=byte_offset)


def _range(value: _native.NativeAstRange) -> AstRange:
    start, end = value
    return AstRange(start=_position(start), end=_position(end))


def _capture(value: _native.NativeAstCapture) -> AstCapture:
    name, text, capture_range = value
    return AstCapture(name=name, text=text, range=None if capture_range is None else _range(capture_range))


def _match(value: _native.NativeAstMatch) -> AstMatch:
    path, language, text, match_range, captures = value
    return AstMatch(
        path=path,
        language=language,
        text=text,
        range=_range(match_range),
        captures=tuple(_capture(capture) for capture in captures),
    )


def _change(value: _native.NativeAstChange) -> AstChange:
    path, language, before, after, change_range = value
    return AstChange(path=path, language=language, before=before, after=after, range=_range(change_range))


def _file_change(value: _native.NativeAstFileChange) -> AstFileChange:
    path, original_sha256, updated_sha256, replacements = value
    return AstFileChange(
        path=path,
        original_sha256=original_sha256,
        updated_sha256=updated_sha256,
        replacements=replacements,
    )
