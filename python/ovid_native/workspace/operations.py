from enum import StrEnum
from typing import TYPE_CHECKING

from ovid_core.services import AgentServiceKey, AgentServiceRef


if TYPE_CHECKING:
    from ovid_native.workspace.models import WorkspaceSession


WORKSPACE_SERVICE_KEY: AgentServiceKey[WorkspaceSession] = AgentServiceKey(
    id='ovid_native.workspace',
    api_version=2,
    value_type=None,
)


def workspace_ref(name: str = 'default') -> AgentServiceRef[WorkspaceSession]:
    return AgentServiceRef(key=WORKSPACE_SERVICE_KEY, name=name)


class WorkspaceOperation(StrEnum):
    FILES = 'files'
    COMMAND = 'command'
    SEARCH = 'search'
    AST = 'ast'
    FFF = 'fff'
    OBSERVATIONS = 'observations'
    CHANGE_EVENTS = 'change_events'
    VIEW = 'view'

    @classmethod
    def native_defaults(cls, *, has_view: bool) -> frozenset[WorkspaceOperation]:
        operations = frozenset(
            (
                cls.FILES,
                cls.COMMAND,
                cls.SEARCH,
                cls.AST,
                cls.FFF,
                cls.OBSERVATIONS,
                cls.CHANGE_EVENTS,
            )
        )
        if has_view:
            return operations | {cls.VIEW}

        return operations
