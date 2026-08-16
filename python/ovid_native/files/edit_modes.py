from collections.abc import Callable
from enum import StrEnum
from threading import Lock

from ovid_core.models import BaseModel
from pydantic import Field

from ovid_native import _native
from ovid_native.workspace.errors import WorkspaceClosedError, WorkspaceEditModeError


class EditMode(StrEnum):
    REPLACE = 'replace'
    PATCH = 'patch'
    APPLY_PATCH = 'apply_patch'


class EditModeSelection(BaseModel):
    mode: str
    generation: int = Field(ge=1)


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
        self._next_listener = 0

    @property
    def current(self) -> EditModeSelection:
        self._ensure_open()
        mode, generation = _native.workspace_edit_mode(self._workspace)
        return EditModeSelection(mode=mode, generation=generation)

    def set(self, mode: EditMode | str) -> EditModeSelection:
        self._ensure_open()
        try:
            selected = EditMode(mode)
        except ValueError as error:
            raise WorkspaceEditModeError(f'Workspace edit mode is not registered: {mode}') from error
        previous = self.current
        try:
            selected_mode, generation = _native.workspace_set_edit_mode(self._workspace, selected.value)
        except _native.NativeWorkspaceEditModeError as error:
            raise WorkspaceEditModeError(str(error)) from error
        selection = EditModeSelection(mode=selected_mode, generation=generation)
        if selection != previous:
            with self._listener_lock:
                listeners = tuple(self._listeners.values())
            for listener in listeners:
                listener(selection)
        return selection

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
