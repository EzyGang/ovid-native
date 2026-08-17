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
    from ovid_native.files.edit_modes import EditModeProvider, EditModeState
    from ovid_native.files.models import (
        ApplyPatchEditRequest,
        HashlineEditRequest,
        PatchEditRequest,
        ReplaceEditRequest,
        WorkspaceCreateRequest,
        WorkspaceDeleteRequest,
        WorkspaceDirectoryReadRequest,
        WorkspaceEditResult,
        WorkspaceFileReadRequest,
        WorkspaceMoveRequest,
        WorkspaceReadDirectoryResult,
        WorkspaceReadFileResult,
        WorkspaceReadRequest,
        WorkspaceReadResult,
        WorkspaceReplaceRequest,
        WorkspaceWriteResult,
    )
    from ovid_native.search.models import GlobRequest, GlobResult, GrepRequest, GrepResult
    from ovid_native.workspace.observations import WorkspaceChangeEvents, WorkspaceObservationService
    from ovid_native.workspace.policy import WorkspacePolicyState
from ovid_native.workspace.operations import WorkspaceOperation


class WorkspaceSessionId(BaseRootModel[str]):
    pass


class WorkspaceMutation(Protocol):
    @property
    def mode(self) -> str: ...

    @property
    def mode_generation(self) -> int: ...

    @property
    def policy_generation(self) -> int: ...


class WorkspaceFilesProvider(Protocol):
    async def read_file(self, request: WorkspaceFileReadRequest) -> WorkspaceReadFileResult: ...

    async def list_directory(self, request: WorkspaceDirectoryReadRequest) -> WorkspaceReadDirectoryResult: ...

    async def create_file(self, request: WorkspaceCreateRequest) -> WorkspaceWriteResult: ...

    async def replace_file(self, request: WorkspaceReplaceRequest) -> WorkspaceWriteResult: ...

    async def delete_file(self, request: WorkspaceDeleteRequest) -> WorkspaceWriteResult: ...

    async def move_file(self, request: WorkspaceMoveRequest) -> WorkspaceWriteResult: ...

    async def read(self, request: WorkspaceReadRequest) -> WorkspaceReadResult: ...

    async def replace(
        self,
        request: ReplaceEditRequest,
        *,
        mutation: WorkspaceMutation | None = None,
    ) -> WorkspaceEditResult: ...

    async def patch(
        self,
        request: PatchEditRequest,
        *,
        mutation: WorkspaceMutation | None = None,
    ) -> WorkspaceEditResult: ...

    async def apply_patch(
        self,
        request: ApplyPatchEditRequest,
        *,
        mutation: WorkspaceMutation | None = None,
    ) -> WorkspaceEditResult: ...

    async def hashline(
        self,
        request: HashlineEditRequest,
        *,
        mutation: WorkspaceMutation | None = None,
    ) -> WorkspaceEditResult: ...


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
    def edit_mode_providers(self) -> tuple[EditModeProvider, ...]: ...

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
    def view(self) -> WorkspaceViewProvider: ...

    @property
    def fff(self) -> WorkspaceFffProvider: ...

    async def close(self) -> None: ...
