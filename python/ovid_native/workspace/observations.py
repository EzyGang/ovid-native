from collections.abc import Callable
from typing import Literal, Protocol

from ovid_core.models import BaseModel
from pydantic import Field

from ovid_native import _native
from ovid_native._native_execution import run_native
from ovid_native.workspace.errors import _NATIVE_ERRORS, WorkspaceStaleError, translate_native_workspace_error
from ovid_native.workspace.events import NativeWorkspaceChangeEvents as NativeWorkspaceChangeEvents
from ovid_native.workspace.events import WorkspaceChangeEvent as WorkspaceChangeEvent
from ovid_native.workspace.events import WorkspaceChangeEvents as WorkspaceChangeEvents
from ovid_native.workspace.events import WorkspaceChangeSubscription as WorkspaceChangeSubscription
from ovid_native.workspace.models import WorkspaceFilesProvider, WorkspaceSessionId


class WorkspaceLineRange(BaseModel):
    start: int = Field(ge=1)
    end: int = Field(ge=1)


class WorkspaceObservedLine(BaseModel):
    line_number: int = Field(ge=1)
    short_hash: str = Field(pattern=r'^[0-9A-F]{2}$')
    content_sha256: str = Field(pattern=r'^[0-9a-f]{64}$')


class WorkspaceRenderedLine(BaseModel):
    line_number: int = Field(ge=1)
    short_hash: str = Field(pattern=r'^(?:[0-9A-F]{2}|--)$')
    text: str


class WorkspaceObservationReceipt(BaseModel):
    session_id: WorkspaceSessionId
    path: str
    tag: str = Field(pattern=r'^[0-9A-F]{4}$')
    content_sha256: str = Field(pattern=r'^[0-9a-f]{64}$')
    generation: int = Field(ge=1)
    visible_ranges: tuple[WorkspaceLineRange, ...]
    complete_presentation: bool


class WorkspaceObservationRequest(BaseModel):
    path: str = Field(min_length=1)
    expected_revision: str | None = None
    visible_ranges: tuple[WorkspaceLineRange, ...]
    purpose: Literal['read'] = 'read'


class WorkspaceLineValidationRequest(BaseModel):
    path: str = Field(min_length=1)
    tag: str = Field(pattern=r'^[0-9A-Fa-f]{4}$')
    line_numbers: tuple[int, ...] = Field(min_length=1)


class WorkspaceLineValidationResult(BaseModel):
    observation: WorkspaceObservationReceipt
    valid: Literal[True] = True


class ObservedWorkspaceFile(BaseModel):
    path: str
    observation: WorkspaceObservationReceipt | None
    lines: tuple[WorkspaceRenderedLine, ...]
    total_lines: int = Field(ge=0)
    complete_presentation: bool
    editable: bool


class WorkspaceObservationService(Protocol):
    @property
    def session_id(self) -> WorkspaceSessionId: ...
    async def observe_file(self, request: WorkspaceObservationRequest) -> ObservedWorkspaceFile: ...

    async def resolve_observation(self, path: str, tag: str) -> WorkspaceObservationReceipt: ...

    async def validate_observed_lines(
        self,
        request: WorkspaceLineValidationRequest,
    ) -> WorkspaceLineValidationResult: ...

    async def observe_claims(
        self,
        *,
        path: str,
        claims: tuple[tuple[int, str], ...],
        spans: tuple[tuple[int, int, int, int], ...],
        complete_presentation: bool,
    ) -> ObservedWorkspaceFile: ...


class WorkspaceObservationStore(Protocol):
    def bind(
        self,
        *,
        session_id: WorkspaceSessionId,
        files: WorkspaceFilesProvider,
    ) -> WorkspaceObservationService: ...


class NativeWorkspaceObservationService:
    def __init__(self, workspace: _native.NativeWorkspace, *, session_id: WorkspaceSessionId) -> None:
        self._workspace = workspace
        self._session_id = session_id

    @property
    def session_id(self) -> WorkspaceSessionId:
        return self._session_id

    async def observe_file(self, request: WorkspaceObservationRequest) -> ObservedWorkspaceFile:
        if request.expected_revision is not None:
            revision = str(_native.workspace_revision(self._workspace))
            if revision != request.expected_revision:
                message = f'Workspace revision changed: expected {request.expected_revision}, got {revision}'
                raise WorkspaceStaleError(message)
        native_ranges: list[tuple[int, int | None]] = [
            (line_range.start, line_range.end) for line_range in request.visible_ranges
        ]
        native = await self._call(lambda: _native.workspace_read_file(self._workspace, request.path, native_ranges))
        path, receipt, lines, total_lines, complete, editable, _, _, _ = native
        return ObservedWorkspaceFile(
            path=path,
            observation=None if receipt is None else self._receipt(receipt),
            lines=tuple(WorkspaceRenderedLine(line_number=line[0], short_hash=line[1], text=line[2]) for line in lines),
            total_lines=total_lines,
            complete_presentation=complete,
            editable=editable,
        )

    async def observe_claims(
        self,
        *,
        path: str,
        claims: tuple[tuple[int, str], ...],
        spans: tuple[tuple[int, int, int, int], ...],
        complete_presentation: bool,
    ) -> ObservedWorkspaceFile:
        receipt, lines = await self._call(
            lambda: _native.workspace_observe_source_lines(
                self._workspace,
                path,
                list(claims),
                list(spans),
                complete_presentation,
            )
        )
        return ObservedWorkspaceFile(
            path=path,
            observation=self._receipt(receipt),
            lines=tuple(WorkspaceRenderedLine(line_number=line[0], short_hash=line[1], text=line[2]) for line in lines),
            total_lines=len(lines),
            complete_presentation=complete_presentation,
            editable=True,
        )

    async def resolve_observation(self, path: str, tag: str) -> WorkspaceObservationReceipt:
        native = await self._call(lambda: _native.workspace_resolve_observation(self._workspace, path, tag))
        return self._receipt(native)

    async def validate_observed_lines(
        self,
        request: WorkspaceLineValidationRequest,
    ) -> WorkspaceLineValidationResult:
        native = await self._call(
            lambda: _native.workspace_validate_observed_lines(
                self._workspace,
                request.path,
                request.tag,
                list(request.line_numbers),
            )
        )
        return WorkspaceLineValidationResult(observation=self._receipt(native))

    def _receipt(self, native: _native.NativeWorkspaceObservationReceipt) -> WorkspaceObservationReceipt:
        path, tag, content_sha256, generation, ranges, complete = native
        return WorkspaceObservationReceipt(
            session_id=self._session_id,
            path=path,
            tag=tag,
            content_sha256=content_sha256,
            generation=generation,
            visible_ranges=tuple(WorkspaceLineRange(start=start, end=end) for start, end in ranges),
            complete_presentation=complete,
        )

    async def _call[Result](self, operation: Callable[[], Result]) -> Result:
        try:
            return await run_native(operation)
        except _NATIVE_ERRORS as error:
            raise translate_native_workspace_error(error) from error
