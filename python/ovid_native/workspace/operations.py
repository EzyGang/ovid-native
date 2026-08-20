from enum import StrEnum
from typing import TYPE_CHECKING

from ovid_core.services import AgentServiceKey, AgentServiceRef


if TYPE_CHECKING:
    from ovid_native.workspace.models import WorkspaceSession


WORKSPACE_SERVICE_KEY: AgentServiceKey[WorkspaceSession] = AgentServiceKey(
    id='ovid_native.workspace',
    api_version=1,
    value_type=None,
)


def workspace_ref(name: str = 'default') -> AgentServiceRef[WorkspaceSession]:
    return AgentServiceRef(key=WORKSPACE_SERVICE_KEY, name=name)


class WorkspaceOperation(StrEnum):
    FILES = 'files'
    SEARCH = 'search'
    AST = 'ast'
    FFF = 'fff'
    OBSERVATIONS = 'observations'
    CHANGE_EVENTS = 'change_events'
    VIEW = 'view'
