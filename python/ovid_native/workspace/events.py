from collections.abc import Callable
from typing import Literal, Protocol

from pydantic import Field

from ovid_native.workspace._subscriptions import CallbackSubscription, ListenerRegistry
from ovid_native.workspace.models import WorkspacePathChange, WorkspaceSessionId


class WorkspaceChangeEvent(WorkspacePathChange):
    session_id: WorkspaceSessionId
    generation: int = Field(ge=1)


type WorkspaceChangeListener = Callable[[WorkspaceChangeEvent], None]


class WorkspaceChangeSubscription(CallbackSubscription):
    pass


class WorkspaceChangeEvents(Protocol):
    def subscribe(self, listener: WorkspaceChangeListener) -> WorkspaceChangeSubscription: ...


class NativeWorkspaceChangeEvents:
    def __init__(self, *, session_id: WorkspaceSessionId) -> None:
        self._session_id = session_id
        self._listeners = ListenerRegistry[WorkspaceChangeEvent]()

    def subscribe(self, listener: WorkspaceChangeListener) -> WorkspaceChangeSubscription:
        return self._listeners.subscribe(listener, WorkspaceChangeSubscription)

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
        self._listeners.publish(event)
