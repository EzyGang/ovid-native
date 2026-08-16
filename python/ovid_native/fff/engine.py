from collections.abc import Callable
from pathlib import Path
from types import TracebackType
from typing import Self

from ovid_native import _native
from ovid_native._native_execution import run_native
from ovid_native.fff import _mapping
from ovid_native.fff.errors import (
    FffCancelledError,
    FffClosedError,
    FffConfigurationError,
    FffError,
    FffIndexNotReadyError,
    FffLimitError,
    FffPathError,
    FffPatternError,
    FffQueryError,
    FffRuntimeError,
    FffStartupError,
)
from ovid_native.fff.models import (
    FffConfig,
    FffConstraints,
    FffFindRequest,
    FffFindResult,
    FffGrepRequest,
    FffGrepResult,
    FffIndexStatus,
    FffLimits,
    FffMultiGrepRequest,
)
from ovid_native.runtime import ensure_native_compatibility
from ovid_native.workspace.errors import WorkspaceClosedError


_NATIVE_ERRORS: tuple[type[Exception], ...] = (
    _native.NativeFffConfigurationError,
    _native.NativeFffPathError,
    _native.NativeFffQueryError,
    _native.NativeFffPatternError,
    _native.NativeFffLimitError,
    _native.NativeFffIndexNotReadyError,
    _native.NativeFffClosedError,
    _native.NativeFffCancelledError,
    _native.NativeFffRuntimeError,
    _native.NativeFffStartupError,
)


class FffEngine:
    def __init__(
        self,
        *,
        root: Path,
        config: FffConfig = FffConfig(),
        limits: FffLimits = FffLimits(),
    ) -> None:
        ensure_native_compatibility()
        try:
            workspace = _native.workspace_create(str(root))
        except ValueError as error:
            raise FffConfigurationError(str(error)) from error

        self._initialize(workspace, config=config, limits=limits)

    @classmethod
    def _from_workspace(
        cls,
        workspace: _native.NativeWorkspace,
        *,
        config: FffConfig = FffConfig(),
        limits: FffLimits = FffLimits(),
    ) -> Self:
        engine = cls.__new__(cls)
        engine._initialize(workspace, config=config, limits=limits)
        return engine

    def _initialize(
        self,
        workspace: _native.NativeWorkspace,
        *,
        config: FffConfig,
        limits: FffLimits,
    ) -> None:
        self._workspace = workspace
        self._config = config
        self._limits = limits
        self._indexed_revision: int | None = None
        self._native = self._call(
            lambda: _native.fff_create(
                workspace,
                _native.NativeFffConfig(
                    config.watch,
                    config.enable_content_indexing,
                    config.enable_mmap_cache,
                    config.initial_scan_timeout_seconds,
                    config.search_timeout_seconds,
                ),
                _native.NativeFffLimits(
                    limits.max_results,
                    limits.max_matches_per_file,
                    limits.max_patterns,
                    limits.max_pattern_characters,
                    limits.max_query_characters,
                    limits.max_file_bytes,
                    limits.max_context_lines,
                    limits.max_search_timeout_seconds,
                ),
            )
        )

    @property
    def config(self) -> FffConfig:
        return self._config

    async def start(self) -> FffIndexStatus:
        self._ensure_open()
        value = await run_native(lambda: self._call(lambda: _native.fff_start(self._native)))
        return _mapping.index_status(value)

    async def wait_ready(self, *, timeout_seconds: float | None = None) -> FffIndexStatus:
        self._ensure_open()
        timeout = timeout_seconds if timeout_seconds is not None else self._config.initial_scan_timeout_seconds
        value = await run_native(lambda: self._call(lambda: _native.fff_wait_ready(self._native, timeout)))
        self._indexed_revision = _native.workspace_revision(self._workspace)
        return _mapping.index_status(value)

    async def status(self) -> FffIndexStatus:
        self._ensure_open()
        value = await run_native(lambda: self._call(lambda: _native.fff_status(self._native)))
        return _mapping.index_status(value)

    async def rescan(self) -> FffIndexStatus:
        self._ensure_open()
        value = await run_native(lambda: self._call(lambda: _native.fff_rescan(self._native)))
        return _mapping.index_status(value)

    async def close(self) -> None:
        await run_native(lambda: self._call(lambda: _native.fff_close(self._native)))

    async def find(self, request: FffFindRequest) -> FffFindResult:
        await self._refresh_if_stale()
        constraints = self._constraints(request.constraints)
        native_request = _native.NativeFffFindRequest(
            request.query,
            constraints,
            request.kind,
            request.offset,
            request.limit,
        )
        value = await run_native(lambda: self._call(lambda: _native.fff_find(self._native, native_request)))
        result = _mapping.find_result(value)
        self._indexed_revision = _native.workspace_revision(self._workspace)
        return result

    async def grep(self, request: FffGrepRequest) -> FffGrepResult:
        await self._refresh_if_stale()
        cancellation = _native.NativeFffCancellation()
        native_request = _native.NativeFffGrepRequest(
            request.query,
            self._constraints(request.constraints),
            (request.mode, request.smart_case),
            (request.file_offset, request.limit, request.matches_per_file),
            (
                request.context_before,
                request.context_after,
                request.max_file_bytes,
                request.timeout_seconds,
                request.classify_definitions,
            ),
        )
        value = await run_native(
            lambda: self._call(lambda: _native.fff_grep(self._native, native_request, cancellation)),
            cancellation=cancellation,
        )
        result = _mapping.grep_result(value)
        self._indexed_revision = _native.workspace_revision(self._workspace)
        return result

    async def multi_grep(self, request: FffMultiGrepRequest) -> FffGrepResult:
        await self._refresh_if_stale()
        cancellation = _native.NativeFffCancellation()
        native_request = _native.NativeFffMultiGrepRequest(
            list(request.patterns),
            self._constraints(request.constraints),
            request.smart_case,
            (request.file_offset, request.limit, request.matches_per_file),
            (
                request.context_before,
                request.context_after,
                request.max_file_bytes,
                request.timeout_seconds,
                request.classify_definitions,
            ),
        )
        value = await run_native(
            lambda: self._call(lambda: _native.fff_multi_grep(self._native, native_request, cancellation)),
            cancellation=cancellation,
        )
        result = _mapping.grep_result(value)
        self._indexed_revision = _native.workspace_revision(self._workspace)
        return result

    async def __aenter__(self) -> Self:
        await self.start()
        return self

    async def __aexit__(
        self,
        exception_type: type[BaseException] | None,
        exception: BaseException | None,
        traceback: TracebackType | None,
    ) -> None:
        await self.close()

    @staticmethod
    def _constraints(value: FffConstraints) -> tuple[list[str], list[str], str | None]:
        return list(value.include), list(value.exclude), value.git_status

    async def _refresh_if_stale(self) -> None:
        self._ensure_open()
        revision = _native.workspace_revision(self._workspace)
        if self._indexed_revision is None or self._indexed_revision == revision:
            return

        await run_native(lambda: self._call(lambda: _native.fff_rescan(self._native)))
        await self.wait_ready()

    def _ensure_open(self) -> None:
        if _native.workspace_is_closed(self._workspace):
            raise WorkspaceClosedError('Workspace session is closed')

    @staticmethod
    def _call[Result](operation: Callable[[], Result]) -> Result:
        try:
            return operation()
        except _NATIVE_ERRORS as error:
            raise _public_error(error) from error


def _public_error(error: Exception) -> FffError:
    mappings: tuple[tuple[type[Exception], type[FffError]], ...] = (
        (_native.NativeFffConfigurationError, FffConfigurationError),
        (_native.NativeFffPathError, FffPathError),
        (_native.NativeFffQueryError, FffQueryError),
        (_native.NativeFffPatternError, FffPatternError),
        (_native.NativeFffLimitError, FffLimitError),
        (_native.NativeFffIndexNotReadyError, FffIndexNotReadyError),
        (_native.NativeFffClosedError, FffClosedError),
        (_native.NativeFffCancelledError, FffCancelledError),
        (_native.NativeFffStartupError, FffStartupError),
    )
    for native_type, public_type in mappings:
        if isinstance(error, native_type):
            return public_type(str(error))
    return FffRuntimeError(str(error))
