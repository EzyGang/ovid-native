from typing import Protocol

from ovid_core.services import AgentServiceBinding, AgentServiceKey, AgentServiceRef, AgentServiceRequirement

from ovid_native.workspace.models import WorkspaceOperation, WorkspaceSessionId
from ovid_native.workspace.operations import WorkspaceAstProvider, WorkspaceFffProvider, WorkspaceSearchProvider


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


WORKSPACE_SERVICE_KEY = AgentServiceKey[WorkspaceSession](
    id='ovid_native.workspace',
    api_version=1,
    value_type=None,
)


def workspace_ref(name: str = 'default') -> AgentServiceRef[WorkspaceSession]:
    return AgentServiceRef(key=WORKSPACE_SERVICE_KEY, name=name)


def workspace_binding(
    session: WorkspaceSession,
    *,
    name: str = 'default',
) -> AgentServiceBinding[WorkspaceSession]:
    return AgentServiceBinding(
        ref=workspace_ref(name),
        value=session,
        provider=type(session).__qualname__,
        features=frozenset(operation.value for operation in session.operations),
        identity=session.id.root,
    )


def _workspace_requirement(operation: WorkspaceOperation, *, name: str) -> AgentServiceRequirement:
    return AgentServiceRequirement(
        service_id=WORKSPACE_SERVICE_KEY.id,
        api_version=WORKSPACE_SERVICE_KEY.api_version,
        name=name,
        required_features=frozenset({operation.value}),
    )
