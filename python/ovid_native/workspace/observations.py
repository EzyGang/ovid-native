from collections.abc import Callable
from threading import Lock
from typing import Literal, Protocol

from ovid_core.models import BaseModel
from pydantic import Field

from ovid_native import _native
from ovid_native._native_execution import run_native
from ovid_native.workspace.errors import _NATIVE_ERRORS, WorkspaceStaleError, translate_native_workspace_error
from ovid_native.workspace.models import WorkspaceSessionId


class WorkspaceLineRange(BaseModel):
    start: int = Field(ge=1)
    end: int = Field(ge=1)


class WorkspaceObservedLine(BaseModel):
    line_number: int = Field(ge=1)
    short_hash: str = Field(pattern=r'^[0-9A-F]{2}$')
    content_sha256: str = Field(pattern=r'^[0-9a-f]{64}$')


class WorkspaceRenderedLine(BaseModel):
    line_number: int = Field(ge=1)
    short_hash: str = Field(pattern=r'^[0-9A-F]{2}$')
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


class WorkspaceChangeEvent(BaseModel):
    session_id: WorkspaceSessionId
    path: str
    operation: Literal['create', 'update', 'delete', 'move']
    destination: str | None = None
    generation: int = Field(ge=1)
    revision: int = Field(ge=1)


type WorkspaceChangeListener = Callable[[WorkspaceChangeEvent], None]


class WorkspaceChangeSubscription:
    def __init__(self, cancel: Callable[[], None]) -> None:
        self._cancel = cancel
        self._closed = False
        self._lock = Lock()

    def close(self) -> None:
        with self._lock:
            if self._closed:
                return
            self._closed = True
        self._cancel()


class WorkspaceChangeEvents(Protocol):
    def subscribe(self, listener: WorkspaceChangeListener) -> WorkspaceChangeSubscription: ...


class NativeWorkspaceChangeEvents:
    def __init__(self, *, session_id: WorkspaceSessionId) -> None:
        self._session_id = session_id
        self._listeners: dict[int, WorkspaceChangeListener] = {}
        self._lock = Lock()
        self._next_listener = 0

    def subscribe(self, listener: WorkspaceChangeListener) -> WorkspaceChangeSubscription:
        with self._lock:
            listener_id = self._next_listener
            self._next_listener += 1
            self._listeners[listener_id] = listener
        return WorkspaceChangeSubscription(lambda: self._remove(listener_id))

    def publish(
        self,
        *,
        path: str,
        operation: Literal['create', 'update', 'delete', 'move'],
        destination: str | None,
        generation: int,
        revision: int,
    ) -> None:
        event = WorkspaceChangeEvent(
            session_id=self._session_id,
            path=path,
            operation=operation,
            destination=destination,
            generation=generation,
            revision=revision,
        )
        with self._lock:
            listeners = tuple(self._listeners.values())
        for listener in listeners:
            listener(event)

    def _remove(self, listener_id: int) -> None:
        with self._lock:
            self._listeners.pop(listener_id, None)


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
    async def observe_file(self, request: WorkspaceObservationRequest) -> ObservedWorkspaceFile: ...

    async def resolve_observation(self, path: str, tag: str) -> WorkspaceObservationReceipt: ...

    async def validate_observed_lines(
        self,
        request: WorkspaceLineValidationRequest,
    ) -> WorkspaceLineValidationResult: ...


class NativeWorkspaceObservationService:
    def __init__(self, workspace: _native.NativeWorkspace, *, session_id: WorkspaceSessionId) -> None:
        self._workspace = workspace
        self._session_id = session_id

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
        path, receipt, lines, total_lines, complete, editable, _, _ = native
        return ObservedWorkspaceFile(
            path=path,
            observation=None if receipt is None else self.receipt(receipt),
            lines=tuple(WorkspaceRenderedLine(line_number=line[0], short_hash=line[1], text=line[2]) for line in lines),
            total_lines=total_lines,
            complete_presentation=complete,
            editable=editable,
        )

    async def resolve_observation(self, path: str, tag: str) -> WorkspaceObservationReceipt:
        native = await self._call(lambda: _native.workspace_resolve_observation(self._workspace, path, tag))
        return self.receipt(native)

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
        return WorkspaceLineValidationResult(observation=self.receipt(native))

    def receipt(self, native: _native.NativeWorkspaceObservationReceipt) -> WorkspaceObservationReceipt:
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
