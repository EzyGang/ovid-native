from abc import ABC, abstractmethod
from dataclasses import dataclass, field, replace
from typing import Self

from ovid_core.capabilities.base import BaseCapability, CapabilityContributions
from ovid_core.services import AgentServiceRequirement, AgentServices

from ovid_native.workspace.models import WorkspaceSession
from ovid_native.workspace.operations import WORKSPACE_SERVICE_KEY, WorkspaceOperation, workspace_ref


@dataclass(frozen=True, slots=True, kw_only=True)
class WorkspaceCapability[Deps](BaseCapability[Deps], ABC):
    workspace: str = 'default'
    contributions: CapabilityContributions[Deps] = field(init=False, repr=False)
    requirements: tuple[AgentServiceRequirement, ...] = field(init=False)

    def __post_init__(self) -> None:
        operations = self._required_operations()
        object.__setattr__(self, 'contributions', CapabilityContributions())
        object.__setattr__(
            self,
            'requirements',
            (
                AgentServiceRequirement(
                    service_id=WORKSPACE_SERVICE_KEY.id,
                    api_version=WORKSPACE_SERVICE_KEY.api_version,
                    name=self.workspace,
                    required_features=frozenset(operation.value for operation in operations),
                ),
            ),
        )

    def bind(self, services: AgentServices) -> Self:
        super().bind(services)
        session = services.resolve(workspace_ref(self.workspace))
        bound = replace(self)
        object.__setattr__(bound, 'contributions', self._contribute(session))
        return bound

    @abstractmethod
    def _required_operations(self) -> frozenset[WorkspaceOperation]: ...

    @abstractmethod
    def _contribute(self, session: WorkspaceSession) -> CapabilityContributions[Deps]: ...
