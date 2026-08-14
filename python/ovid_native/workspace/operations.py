from typing import TYPE_CHECKING, Protocol


if TYPE_CHECKING:
    from contextlib import AbstractAsyncContextManager

    from ovid_native.ast.models import (
        AstRewriteApplyRequest,
        AstRewriteApplyResult,
        AstRewritePreview,
        AstRewritePreviewRequest,
        AstSearchRequest,
        AstSearchResult,
    )
    from ovid_native.fff.models import (
        FffFindRequest,
        FffFindResult,
        FffGrepRequest,
        FffGrepResult,
        FffIndexStatus,
        FffMultiGrepRequest,
    )
    from ovid_native.search.models import GlobRequest, GlobResult, GrepRequest, GrepResult
    from ovid_native.workspace.models import WorkspaceView, WorkspaceViewPurpose


class WorkspaceSearchProvider(Protocol):
    async def glob(self, request: GlobRequest) -> GlobResult: ...

    async def grep(self, request: GrepRequest) -> GrepResult: ...


class WorkspaceAstProvider(Protocol):
    async def search(self, request: AstSearchRequest) -> AstSearchResult: ...

    async def preview_rewrite(self, request: AstRewritePreviewRequest) -> AstRewritePreview: ...

    async def apply_rewrite(self, request: AstRewriteApplyRequest) -> AstRewriteApplyResult: ...


class WorkspaceFffProvider(Protocol):
    async def start(self) -> FffIndexStatus: ...

    async def find(self, request: FffFindRequest) -> FffFindResult: ...

    async def grep(self, request: FffGrepRequest) -> FffGrepResult: ...

    async def multi_grep(self, request: FffMultiGrepRequest) -> FffGrepResult: ...

    async def close(self) -> None: ...


class WorkspaceViewProvider(Protocol):
    async def acquire_view(
        self,
        purpose: WorkspaceViewPurpose,
    ) -> AbstractAsyncContextManager[WorkspaceView]: ...
