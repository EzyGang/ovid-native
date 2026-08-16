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
    from ovid_native.files.edit_modes import EditModeState
    from ovid_native.files.models import (
        WorkspaceCreateRequest,
        WorkspaceDeleteRequest,
        WorkspaceDirectoryReadRequest,
        WorkspaceFileReadRequest,
        WorkspaceMoveRequest,
        WorkspaceReadDirectoryResult,
        WorkspaceReadFileResult,
        WorkspaceReplaceRequest,
        WorkspaceWriteResult,
    )
    from ovid_native.search.models import GlobRequest, GlobResult, GrepRequest, GrepResult
    from ovid_native.workspace.observations import WorkspaceChangeEvents, WorkspaceObservationService
    from ovid_native.workspace.policy import WorkspacePolicyState
from ovid_native.workspace.operations import WorkspaceOperation


class WorkspaceSessionId(BaseRootModel[str]):
    pass


class WorkspaceFilesProvider(Protocol):
    async def read_file(self, request: WorkspaceFileReadRequest) -> WorkspaceReadFileResult: ...

    async def list_directory(self, request: WorkspaceDirectoryReadRequest) -> WorkspaceReadDirectoryResult: ...

    async def create_file(self, request: WorkspaceCreateRequest) -> WorkspaceWriteResult: ...

    async def replace_file(self, request: WorkspaceReplaceRequest) -> WorkspaceWriteResult: ...

    async def delete_file(self, request: WorkspaceDeleteRequest) -> WorkspaceWriteResult: ...

    async def move_file(self, request: WorkspaceMoveRequest) -> WorkspaceWriteResult: ...


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
    def edit_mode(self) -> EditModeState: ...

    @property
    def policy(self) -> WorkspacePolicyState: ...

    @property
    def files(self) -> WorkspaceFilesProvider: ...

    @property
    def observations(self) -> WorkspaceObservationService: ...

    @property
    def change_events(self) -> WorkspaceChangeEvents: ...

    @property
    def search(self) -> WorkspaceSearchProvider: ...

    @property
    def ast(self) -> WorkspaceAstProvider: ...

    @property
    def fff(self) -> WorkspaceFffProvider: ...

    async def close(self) -> None: ...
