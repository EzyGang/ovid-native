import asyncio
import secrets
import time
from collections.abc import Callable
from dataclasses import dataclass
from datetime import UTC, datetime, timedelta
from pathlib import Path
from typing import Self

from ovid_native import _native
from ovid_native._native_execution import run_native
from ovid_native.ast import _mapping
from ovid_native.ast.errors import (
    AstConfigurationError,
    AstError,
    AstLanguageError,
    AstLimitError,
    AstPathError,
    AstPatternError,
    AstProposalExpiredError,
    AstProposalNotFoundError,
    AstProposalStaleError,
    AstWriteError,
)
from ovid_native.ast.models import (
    AstLanguageInfo,
    AstLimits,
    AstRewriteApplyRequest,
    AstRewriteApplyResult,
    AstRewritePreview,
    AstRewritePreviewRequest,
    AstScanOptions,
    AstSearchRequest,
    AstSearchResult,
)
from ovid_native.runtime import ensure_native_compatibility
from ovid_native.workspace.errors import WorkspaceClosedError


@dataclass(frozen=True, slots=True)
class _Proposal:
    computation: _native.NativeAstRewriteComputation
    session_id: str
    revision: int
    expires_monotonic: float


_NATIVE_ERRORS: tuple[type[Exception], ...] = (
    _native.NativeAstConfigurationError,
    _native.NativeAstPathError,
    _native.NativeAstLanguageError,
    _native.NativeAstPatternError,
    _native.NativeAstLimitError,
    _native.NativeAstProposalStaleError,
    _native.NativeAstWriteError,
)


class AstEngine:
    def __init__(self, *, root: Path, limits: AstLimits | None = None) -> None:
        ensure_native_compatibility()
        try:
            workspace = _native.workspace_create(str(root))
        except ValueError as error:
            raise AstConfigurationError(str(error)) from error

        self._initialize(workspace, session_id=secrets.token_urlsafe(24), limits=limits)
        self._root = root.resolve(strict=True)

    @classmethod
    def _from_workspace(
        cls,
        workspace: _native.NativeWorkspace,
        *,
        session_id: str,
        limits: AstLimits | None = None,
    ) -> Self:
        engine = cls.__new__(cls)
        engine._initialize(workspace, session_id=session_id, limits=limits)
        return engine

    def _initialize(
        self,
        workspace: _native.NativeWorkspace,
        *,
        session_id: str,
        limits: AstLimits | None,
    ) -> None:
        self._workspace = workspace
        self._root = Path(workspace.root)
        self._session_id = session_id
        self._limits = limits if limits is not None else AstLimits()
        self._proposals: dict[str, _Proposal] = {}
        self._proposal_lock = asyncio.Lock()
        self._workspace_write_lock = asyncio.Lock()

    @property
    def root(self) -> Path:
        return self._root

    @property
    def limits(self) -> AstLimits:
        return self._limits

    async def search(self, request: AstSearchRequest) -> AstSearchResult:
        self._ensure_open()
        cancellation = _native.NativeAstCancellation()
        native_request = _native.NativeAstSearchRequest(
            request.pattern,
            _scan_options(request.scan),
            request.language,
            request.strictness,
            (request.offset, request.limit, request.include_captures),
            _limits(self._limits),
            cancellation,
        )
        result = await _call_native(
            lambda: _native.ast_search(self._workspace, native_request),
            cancellation=cancellation,
        )
        return _mapping.search_result(result)

    async def preview_rewrite(self, request: AstRewritePreviewRequest) -> AstRewritePreview:
        self._ensure_open()
        cancellation = _native.NativeAstCancellation()
        native_request = _native.NativeAstRewriteRequest(
            [(operation.pattern, operation.replacement) for operation in request.operations],
            _scan_options(request.scan),
            request.language,
            request.strictness,
            _limits(self._limits),
            cancellation,
        )
        native = await _call_native(
            lambda: _native.ast_preview_rewrite(self._workspace, native_request),
            cancellation=cancellation,
        )
        computation, changes, files, replacements, files_searched, native_issues = native
        expires_at = datetime.now(UTC) + timedelta(seconds=self._limits.proposal_ttl_seconds)
        proposal_id = await self._store(
            computation,
            replacements,
            revision=_native.workspace_revision(self._workspace),
        )

        return AstRewritePreview(
            proposal_id=proposal_id,
            changes=_mapping.changes(changes),
            files=_mapping.file_changes(files),
            total_replacements=replacements,
            files_searched=files_searched,
            expires_at=expires_at,
            issues=_mapping.issues(native_issues),
        )

    async def apply_rewrite(self, request: AstRewriteApplyRequest) -> AstRewriteApplyResult:
        self._ensure_open()
        async with self._workspace_write_lock:
            cancellation = _native.NativeAstCancellation()
            proposal = await self._take(request.proposal_id)
            if proposal.session_id != self._session_id or proposal.revision != _native.workspace_revision(
                self._workspace
            ):
                raise AstProposalStaleError('AST rewrite proposal belongs to an incompatible workspace revision')
            native_files, replacements = await _call_native(
                lambda: _native.ast_apply_rewrite(self._workspace, proposal.computation, cancellation),
                cancellation=cancellation,
            )

        return AstRewriteApplyResult(
            proposal_id=request.proposal_id,
            files=_mapping.file_changes(native_files),
            total_replacements=replacements,
        )

    async def reject_rewrite(self, proposal_id: str) -> bool:
        async with self._proposal_lock:
            now = time.monotonic()
            proposal = self._proposals.pop(proposal_id, None)
            self._purge_expired(now)
            return proposal is not None and proposal.expires_monotonic > now

    async def _store(
        self,
        computation: _native.NativeAstRewriteComputation,
        replacements: int,
        *,
        revision: int,
    ) -> str:
        if replacements == 0:
            return ''

        now = time.monotonic()
        proposal_id = secrets.token_urlsafe(24)
        proposal = _Proposal(
            computation=computation,
            session_id=self._session_id,
            revision=revision,
            expires_monotonic=now + self._limits.proposal_ttl_seconds,
        )
        async with self._proposal_lock:
            self._purge_expired(now)
            while len(self._proposals) >= self._limits.max_pending_proposals:
                del self._proposals[next(iter(self._proposals))]
            self._proposals[proposal_id] = proposal

        return proposal_id

    async def _take(self, proposal_id: str) -> _Proposal:
        async with self._proposal_lock:
            now = time.monotonic()
            proposal = self._proposals.pop(proposal_id, None)
            self._purge_expired(now)

        if proposal is None:
            raise AstProposalNotFoundError(f'AST rewrite proposal not found: {proposal_id}')

        if proposal.expires_monotonic <= now:
            raise AstProposalExpiredError(f'AST rewrite proposal expired: {proposal_id}')

        return proposal

    def _purge_expired(self, now: float) -> None:
        expired = [key for key, proposal in self._proposals.items() if proposal.expires_monotonic <= now]
        for key in expired:
            del self._proposals[key]

    def _ensure_open(self) -> None:
        if _native.workspace_is_closed(self._workspace):
            raise WorkspaceClosedError('Workspace session is closed')


def supported_ast_languages() -> tuple[AstLanguageInfo, ...]:
    ensure_native_compatibility()
    return tuple(_mapping.language_info(value) for value in _native.ast_supported_languages())


def ast_grep_version() -> str:
    ensure_native_compatibility()
    return _native.ast_grep_version()


def _limits(limits: AstLimits) -> _native.NativeAstLimits:
    return _native.NativeAstLimits(
        limits.max_matches,
        limits.max_files,
        limits.max_file_bytes,
        limits.max_replacements,
        limits.max_changed_files,
    )


def _scan_options(options: AstScanOptions) -> _native.NativeAstScanOptions:
    return _native.NativeAstScanOptions(
        list(options.paths),
        options.include_hidden,
        options.respect_gitignore,
        options.include_node_modules,
    )


async def _call_native[Result](
    function: Callable[[], Result],
    *,
    cancellation: _native.NativeAstCancellation | None = None,
) -> Result:
    try:
        return await run_native(function, cancellation=cancellation)
    except _NATIVE_ERRORS as error:
        raise _translate_native(error) from error


def _translate_native(error: Exception) -> AstError:
    mappings: tuple[tuple[type[Exception], type[AstError]], ...] = (
        (_native.NativeAstConfigurationError, AstConfigurationError),
        (_native.NativeAstPathError, AstPathError),
        (_native.NativeAstLanguageError, AstLanguageError),
        (_native.NativeAstPatternError, AstPatternError),
        (_native.NativeAstLimitError, AstLimitError),
        (_native.NativeAstProposalStaleError, AstProposalStaleError),
        (_native.NativeAstWriteError, AstWriteError),
    )
    for native_type, public_type in mappings:
        if isinstance(error, native_type):
            return public_type(str(error))

    return AstError(str(error))
