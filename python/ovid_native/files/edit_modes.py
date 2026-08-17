from collections.abc import Callable
from enum import StrEnum
from threading import Lock
from typing import TYPE_CHECKING, Any, Protocol

from ovid_core.models import BaseModel, BaseRootModel
from ovid_core.tools.base import BaseTool
from pydantic import Field, model_validator

from ovid_native import _native
from ovid_native.files.models import WorkspaceFilesToolResult
from ovid_native.workspace.errors import WorkspaceClosedError, WorkspaceEditModeError
from ovid_native.workspace.operations import WorkspaceOperation


if TYPE_CHECKING:
    from ovid_native.workspace.models import WorkspaceSession


class EditMode(StrEnum):
    HASHLINE = 'hashline'
    REPLACE = 'replace'
    PATCH = 'patch'
    APPLY_PATCH = 'apply_patch'


class EditModeId(BaseRootModel[str]):
    @model_validator(mode='after')
    def validate_namespace(self) -> EditModeId:
        if '.' not in self.root or self.root.startswith('.') or self.root.endswith('.'):
            raise ValueError('custom edit mode IDs must be globally namespaced')
        return self


class EditModeSelection(BaseModel):
    mode: str
    generation: int = Field(ge=1)


class EditModeProvider(Protocol):
    @property
    def id(self) -> str: ...
    @property
    def required_operations(self) -> frozenset[WorkspaceOperation]: ...

    def bind(
        self,
        workspace: WorkspaceSession,
        state: EditModeSelection,
    ) -> BaseTool[Any, Any, WorkspaceFilesToolResult]: ...


type EditModeListener = Callable[[EditModeSelection], None]


class Subscription:
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


class EditModeState:
    def __init__(self, workspace: _native.NativeWorkspace) -> None:
        self._workspace = workspace
        self._listeners: dict[int, EditModeListener] = {}
        self._listener_lock = Lock()
        self._registered = {mode.value for mode in EditMode}
        self._next_listener = 0

    @property
    def current(self) -> EditModeSelection:
        self._ensure_open()
        mode, generation = _native.workspace_edit_mode(self._workspace)
        return EditModeSelection(mode=mode, generation=generation)

    def set(self, mode: EditMode | EditModeId | str) -> EditModeSelection:
        self._ensure_open()
        selected = mode.root if isinstance(mode, EditModeId) else str(mode)
        if selected not in self._registered:
            raise WorkspaceEditModeError(f'Workspace edit mode is not registered: {selected}')
        previous = self.current
        try:
            selected_mode, generation = _native.workspace_set_edit_mode(self._workspace, selected)
        except _native.NativeWorkspaceEditModeError as error:
            raise WorkspaceEditModeError(str(error)) from error
        selection = EditModeSelection(mode=selected_mode, generation=generation)
        if selection != previous:
            with self._listener_lock:
                listeners = tuple(self._listeners.values())
            for listener in listeners:
                listener(selection)
        return selection

    def register(self, mode: EditModeId | str) -> EditModeId:
        self._ensure_open()
        try:
            identifier = mode if isinstance(mode, EditModeId) else EditModeId(mode)
        except ValueError as error:
            raise WorkspaceEditModeError(str(error)) from error
        if identifier.root in self._registered:
            raise WorkspaceEditModeError(f'Workspace edit mode is already registered: {identifier.root}')
        try:
            _native.workspace_register_edit_mode(self._workspace, identifier.root)
        except _native.NativeWorkspaceEditModeError as error:
            raise WorkspaceEditModeError(str(error)) from error
        self._registered.add(identifier.root)
        return identifier

    def subscribe(self, listener: EditModeListener) -> Subscription:
        with self._listener_lock:
            listener_id = self._next_listener
            self._next_listener += 1
            self._listeners[listener_id] = listener
        return Subscription(lambda: self._remove_listener(listener_id))

    def capture(self) -> _native.NativeWorkspaceMutation:
        self._ensure_open()
        return _native.workspace_capture_mutation(self._workspace)

    def _remove_listener(self, listener_id: int) -> None:
        with self._listener_lock:
            self._listeners.pop(listener_id, None)

    def _ensure_open(self) -> None:
        if _native.workspace_is_closed(self._workspace):
            raise WorkspaceClosedError('Workspace session is closed')
