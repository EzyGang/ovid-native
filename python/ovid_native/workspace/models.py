from __future__ import annotations

from contextlib import AbstractAsyncContextManager
from enum import StrEnum
from pathlib import Path
from typing import TYPE_CHECKING, Protocol

from ovid_core.models import BaseModel, BaseRootModel


if TYPE_CHECKING:
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
from ovid_native.workspace.operations import WorkspaceOperation


class WorkspaceSessionId(BaseRootModel[str]):
    pass


class WorkspaceSearchProvider(Protocol):
    async def glob(self, request: GlobRequest) -> GlobResult: ...

    async def grep(self, request: GrepRequest) -> GrepResult: ...


class WorkspaceAstProvider(Protocol):
    async def search(self, request: AstSearchRequest) -> AstSearchResult: ...

    async def preview_rewrite(self, request: AstRewritePreviewRequest) -> AstRewritePreview: ...

    async def apply_rewrite(self, request: AstRewriteApplyRequest) -> AstRewriteApplyResult: ...


class WorkspaceFffProvider(Protocol):
    async def start(self) -> FffIndexStatus: ...
    async def wait_ready(self, *, timeout_seconds: float | None = None) -> FffIndexStatus: ...

    async def find(self, request: FffFindRequest) -> FffFindResult: ...

    async def grep(self, request: FffGrepRequest) -> FffGrepResult: ...

    async def multi_grep(self, request: FffMultiGrepRequest) -> FffGrepResult: ...

    async def close(self) -> None: ...


class WorkspaceViewPurpose(StrEnum):
    SEARCH = 'search'
    AST = 'ast'
    FFF = 'fff'


class WorkspaceView(BaseModel):
    root: Path
    revision: str
    read_only: bool


class WorkspaceViewProvider(Protocol):
    def acquire_view(
        self,
        purpose: WorkspaceViewPurpose,
    ) -> AbstractAsyncContextManager[WorkspaceView]: ...


class WorkspaceSession(Protocol):
    @property
    def id(self) -> WorkspaceSessionId: ...

    @property
    def operations(self) -> frozenset[WorkspaceOperation]: ...

    @property
    def search(self) -> WorkspaceSearchProvider: ...

    @property
    def ast(self) -> WorkspaceAstProvider: ...

    @property
    def fff(self) -> WorkspaceFffProvider: ...

    async def close(self) -> None: ...
