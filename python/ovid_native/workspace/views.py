import asyncio
import difflib
from contextlib import AbstractAsyncContextManager
from dataclasses import dataclass
from datetime import UTC, datetime

from ovid_native.ast.engine import AstEngine
from ovid_native.ast.errors import (
    AstProposalExpiredError,
    AstProposalNotFoundError,
    AstProposalStaleError,
    AstWriteError,
)
from ovid_native.ast.models import (
    AstRewriteApplyRequest,
    AstRewriteApplyResult,
    AstRewritePreview,
    AstRewritePreviewRequest,
    AstSearchRequest,
    AstSearchResult,
)
from ovid_native.fff.engine import FffEngine
from ovid_native.fff.models import (
    FffFindRequest,
    FffFindResult,
    FffGrepRequest,
    FffGrepResult,
    FffIndexStatus,
    FffMultiGrepRequest,
)
from ovid_native.files.edit_modes import EditMode, EditModeState
from ovid_native.files.models import ApplyPatchEditRequest, WorkspaceFileReadRequest
from ovid_native.search.engine import SearchEngine
from ovid_native.search.models import GlobRequest, GlobResult, GrepRequest, GrepResult
from ovid_native.workspace.errors import WorkspaceStaleError
from ovid_native.workspace.models import (
    WorkspaceFilesProvider,
    WorkspaceMutation,
    WorkspaceView,
    WorkspaceViewProvider,
    WorkspaceViewPurpose,
)


class NativeViewSearchProvider:
    def __init__(self, provider: WorkspaceViewProvider) -> None:
        self._provider = provider

    async def glob(self, request: GlobRequest) -> GlobResult:
        async with self._provider.acquire_view(WorkspaceViewPurpose.SEARCH) as view:
            _require_read_only(view)
            return await SearchEngine(root=view.root).glob(request)

    async def grep(self, request: GrepRequest) -> GrepResult:
        async with self._provider.acquire_view(WorkspaceViewPurpose.SEARCH) as view:
            _require_read_only(view)
            return await SearchEngine(root=view.root).grep(request)


@dataclass(frozen=True, slots=True)
class _ViewAstProposal:
    preview: AstRewritePreview
    files: tuple[tuple[str, str, str, int], ...]
    revision: str


class NativeViewAstProvider:
    def __init__(self, provider: WorkspaceViewProvider, files: WorkspaceFilesProvider) -> None:
        self._provider = provider
        self._files = files
        self._proposals: dict[str, _ViewAstProposal] = {}
        self._proposal_lock = asyncio.Lock()
        self._edit_mode: EditModeState | None = None

    def bind_edit_mode(self, state: EditModeState) -> None:
        self._edit_mode = state

    async def search(self, request: AstSearchRequest) -> AstSearchResult:
        async with self._provider.acquire_view(WorkspaceViewPurpose.AST) as view:
            _require_read_only(view)
            return await AstEngine(root=view.root).search(request)

    async def preview_rewrite(self, request: AstRewritePreviewRequest) -> AstRewritePreview:
        async with self._provider.acquire_view(WorkspaceViewPurpose.AST) as view:
            _require_read_only(view)
            engine = AstEngine(root=view.root)
            preview = await engine.preview_rewrite(request)
            if not preview.proposal_id:
                return preview
            files = await engine.proposal_files(preview.proposal_id)
            await engine.reject_rewrite(preview.proposal_id)
        await self._store(
            _ViewAstProposal(
                preview=preview,
                files=files,
                revision=view.revision,
            )
        )
        return preview

    async def apply_rewrite(self, request: AstRewriteApplyRequest) -> AstRewriteApplyResult:
        proposal = await self._take(request.proposal_id)

        async with self._provider.acquire_view(WorkspaceViewPurpose.AST) as view:
            _require_read_only(view)
            if view.revision != proposal.revision:
                raise AstProposalStaleError(
                    f'AST rewrite proposal has a stale workspace revision: {request.proposal_id}'
                )

            patches = await self._patches(proposal)
            patch_body = '\n'.join(patches)
            envelope = f'*** Begin Patch\n{patch_body}\n*** End Patch'
            await self._files.apply_patch(ApplyPatchEditRequest(input=envelope), mutation=self._mutation())

        return AstRewriteApplyResult(
            proposal_id=request.proposal_id,
            files=proposal.preview.files,
            total_replacements=proposal.preview.total_replacements,
        )

    async def _patches(self, proposal: _ViewAstProposal) -> list[str]:
        patches = []
        for path, original_sha256, updated, _ in proposal.files:
            current = await self._files.read_file(WorkspaceFileReadRequest(path=path))
            if current.observation is None or not current.complete_presentation:
                raise AstWriteError(f'AST rewrite requires a complete editable observation: {path}')
            if current.observation.content_sha256 != original_sha256:
                raise AstProposalStaleError(f'AST rewrite proposal is stale for: {path}')
            before = [line.text for line in current.lines]
            after = updated.splitlines()
            diff_lines = list(difflib.unified_diff(before, after, lineterm=''))[2:]
            body = '\n'.join(diff_lines)
            patches.append(f'*** Update File: {path}\n{body}')

        return patches

    async def _store(self, proposal: _ViewAstProposal) -> None:
        async with self._proposal_lock:
            self._purge_expired()
            while len(self._proposals) >= 32:
                del self._proposals[next(iter(self._proposals))]
            self._proposals[proposal.preview.proposal_id] = proposal

    async def _take(self, proposal_id: str) -> _ViewAstProposal:
        async with self._proposal_lock:
            proposal = self._proposals.pop(proposal_id, None)
            self._purge_expired()
        if proposal is None:
            raise AstProposalNotFoundError(f'AST rewrite proposal not found: {proposal_id}')
        if proposal.preview.expires_at <= datetime.now(UTC):
            raise AstProposalExpiredError(f'AST rewrite proposal expired: {proposal_id}')
        return proposal

    def _mutation(self) -> WorkspaceMutation:
        if self._edit_mode is None:
            raise AstWriteError('view-backed AST provider is not bound to an edit mode')
        mutation = self._edit_mode.capture()
        if mutation.mode != EditMode.APPLY_PATCH:
            raise AstWriteError('view-backed AST rewrites require apply_patch edit mode')
        return mutation

    def _purge_expired(self) -> None:
        now = datetime.now(UTC)
        expired = [
            proposal_id for proposal_id, proposal in self._proposals.items() if proposal.preview.expires_at <= now
        ]
        for proposal_id in expired:
            del self._proposals[proposal_id]


class NativeViewFffProvider:
    def __init__(self, provider: WorkspaceViewProvider) -> None:
        self._provider = provider
        self._context: AbstractAsyncContextManager[WorkspaceView] | None = None
        self._engine: FffEngine | None = None
        self._revision = ''
        self._lifecycle_lock = asyncio.Lock()

    async def start(self) -> FffIndexStatus:
        async with self._lifecycle_lock:
            if self._engine is None:
                await self._start()
            else:
                await self._validate_revision()

            engine, _ = self._require_engine()
            return await engine.start()

    async def _start(self) -> None:
        context = self._provider.acquire_view(WorkspaceViewPurpose.FFF)
        view = await context.__aenter__()
        engine: FffEngine | None = None
        try:
            _require_read_only(view)
            engine = FffEngine(root=view.root)
            await engine.start()
        except BaseException:
            try:
                if engine is not None:
                    await engine.close()
            finally:
                await context.__aexit__(None, None, None)
            raise

        self._context = context
        self._revision = view.revision
        self._engine = engine

    async def wait_ready(self, *, timeout_seconds: float | None = None) -> FffIndexStatus:
        engine, _ = self._require_engine()
        await self._validate_revision()
        return await engine.wait_ready(timeout_seconds=timeout_seconds)

    async def find(self, request: FffFindRequest) -> FffFindResult:
        engine, _ = self._require_engine()
        await self._validate_revision()
        return await engine.find(request)

    async def grep(self, request: FffGrepRequest) -> FffGrepResult:
        engine, revision = self._require_engine()
        await self._validate_revision()
        result = await engine.grep(request)
        return result.model_copy(update={'workspace_revision': revision})

    async def multi_grep(self, request: FffMultiGrepRequest) -> FffGrepResult:
        engine, revision = self._require_engine()
        await self._validate_revision()
        result = await engine.multi_grep(request)
        return result.model_copy(update={'workspace_revision': revision})

    async def close(self) -> None:
        async with self._lifecycle_lock:
            engine = self._engine
            context = self._context
            self._engine = None
            self._context = None
            self._revision = ''

            try:
                if engine is not None:
                    await engine.close()
            finally:
                if context is not None:
                    await context.__aexit__(None, None, None)

    def _require_engine(self) -> tuple[FffEngine, str]:
        if self._engine is None:
            raise WorkspaceStaleError('FFF workspace view has not started')
        return self._engine, self._revision

    async def _validate_revision(self) -> None:
        revision = await self._provider.current_revision(WorkspaceViewPurpose.FFF)
        if revision != self._revision:
            raise WorkspaceStaleError('FFF workspace view revision changed; restart the workspace session')


def _require_read_only(view: WorkspaceView) -> None:
    if not view.read_only:
        raise WorkspaceStaleError('native workspace views must be explicitly read-only')
    if not view.root.is_absolute():
        raise WorkspaceStaleError('native workspace view root must be absolute')
