from collections.abc import Callable
from threading import Lock
from typing import Literal, Protocol

from ovid_core.models import BaseModel
from pydantic import Field

from ovid_native.workspace.models import WorkspaceSessionId


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
