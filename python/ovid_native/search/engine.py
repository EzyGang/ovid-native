from collections.abc import Callable
from pathlib import Path

from ovid_native import _native
from ovid_native._native_execution import run_native
from ovid_native.search import _mapping
from ovid_native.search.errors import (
    SearchCancelledError,
    SearchConfigurationError,
    SearchError,
    SearchLimitError,
    SearchPathError,
    SearchPatternError,
    SearchReadError,
)
from ovid_native.search.models import GlobRequest, GlobResult, GrepRequest, GrepResult, SearchLimits


_NATIVE_ERRORS: tuple[type[Exception], ...] = (
    _native.NativeSearchConfigurationError,
    _native.NativeSearchPathError,
    _native.NativeSearchPatternError,
    _native.NativeSearchLimitError,
    _native.NativeSearchCancelledError,
    _native.NativeSearchReadError,
)


class SearchEngine:
    def __init__(self, *, root: Path, limits: SearchLimits | None = None) -> None:
        try:
            workspace = _native.search_workspace(str(root))
        except _NATIVE_ERRORS as error:
            raise _translate_native(error) from error

        self._workspace = workspace
        self._root = Path(workspace.root)
        self._limits = limits if limits is not None else SearchLimits()

    @property
    def root(self) -> Path:
        return self._root

    @property
    def limits(self) -> SearchLimits:
        return self._limits

    async def glob(self, request: GlobRequest) -> GlobResult:
        _validate_glob_limits(request, self._limits)
        cancellation = _native.NativeSearchCancellation()
        native_request = _native.NativeGlobRequest(
            list(request.patterns),
            (request.include_hidden, request.respect_gitignore, request.include_node_modules),
            (
                request.file_type,
                request.order,
                request.limit,
                self._limits.max_scan_files,
                request.timeout_seconds,
            ),
            cancellation,
        )
        result = await _call_native(
            lambda: _native.search_glob(self._workspace, native_request),
            cancellation=cancellation,
        )
        return _mapping.glob_result(result)

    async def grep(self, request: GrepRequest) -> GrepResult:
        _validate_grep_limits(request, self._limits)
        cancellation = _native.NativeSearchCancellation()
        native_request = _native.NativeGrepRequest(
            request.pattern,
            (
                list(request.scan.paths),
                request.scan.include_hidden,
                request.scan.respect_gitignore,
                request.scan.include_node_modules,
            ),
            (request.mode, request.case_sensitive, request.multiline),
            (request.file_offset, request.file_limit, request.matches_per_file),
            (
                request.context_before,
                request.context_after,
                request.max_file_bytes,
                request.large_file_mode,
                request.timeout_seconds,
            ),
            (
                self._limits.max_scan_files,
                self._limits.max_grep_matches,
                self._limits.max_matches_per_file,
                self._limits.max_line_characters,
            ),
            cancellation,
        )
        result = await _call_native(
            lambda: _native.search_grep(self._workspace, native_request),
            cancellation=cancellation,
        )
        return _mapping.grep_result(result)


def _validate_glob_limits(request: GlobRequest, limits: SearchLimits) -> None:
    if request.limit > limits.max_glob_results:
        raise SearchLimitError(f'Glob limit exceeds the engine ceiling of {limits.max_glob_results}')
    if request.timeout_seconds > limits.max_timeout_seconds:
        raise SearchLimitError(f'Glob timeout exceeds the engine ceiling of {limits.max_timeout_seconds} seconds')


def _validate_grep_limits(request: GrepRequest, limits: SearchLimits) -> None:
    checks = (
        (request.file_limit, limits.max_grep_files, 'file limit'),
        (request.matches_per_file, limits.max_matches_per_file, 'matches-per-file limit'),
        (request.max_file_bytes, limits.max_file_bytes, 'file-byte limit'),
        (request.context_before, limits.max_context_lines, 'before-context limit'),
        (request.context_after, limits.max_context_lines, 'after-context limit'),
        (request.timeout_seconds, limits.max_timeout_seconds, 'timeout'),
    )
    for value, ceiling, name in checks:
        if value > ceiling:
            raise SearchLimitError(f'Grep {name} exceeds the engine ceiling of {ceiling}')


async def _call_native[Result](
    function: Callable[[], Result],
    *,
    cancellation: _native.NativeSearchCancellation,
) -> Result:
    try:
        return await run_native(function, cancellation=cancellation)
    except _NATIVE_ERRORS as error:
        raise _translate_native(error) from error


def _translate_native(error: Exception) -> SearchError:
    mappings: tuple[tuple[type[Exception], type[SearchError]], ...] = (
        (_native.NativeSearchConfigurationError, SearchConfigurationError),
        (_native.NativeSearchPathError, SearchPathError),
        (_native.NativeSearchPatternError, SearchPatternError),
        (_native.NativeSearchLimitError, SearchLimitError),
        (_native.NativeSearchCancelledError, SearchCancelledError),
        (_native.NativeSearchReadError, SearchReadError),
    )
    for native_type, public_type in mappings:
        if isinstance(error, native_type):
            return public_type(str(error))

    return SearchError(str(error))
