from dataclasses import dataclass, field
from typing import Self

from ovid_core.capabilities.base import BaseCapability, CapabilityContributions
from ovid_core.services import AgentServiceRequirement, AgentServices

from ovid_native.search.tools import SEARCH_TOOL_INSTRUCTIONS, GlobTool, SearchSourceToolset
from ovid_native.workspace.operations import WORKSPACE_SERVICE_KEY, WorkspaceOperation, workspace_ref


@dataclass(frozen=True, slots=True, kw_only=True)
class SearchCapability[Deps](BaseCapability[Deps]):
    workspace: str = 'default'
    id: str = field(default='native_search', init=False)
    description: str = field(
        default='Fast workspace path discovery and bounded text search',
        init=False,
    )
    defer_loading: bool = field(default=False, init=False)
    contributions: CapabilityContributions[Deps] = field(init=False, repr=False)
    requirements: tuple[AgentServiceRequirement, ...] = field(init=False)

    def __post_init__(self) -> None:
        object.__setattr__(self, 'contributions', CapabilityContributions())
        object.__setattr__(
            self,
            'requirements',
            (
                AgentServiceRequirement(
                    service_id=WORKSPACE_SERVICE_KEY.id,
                    api_version=WORKSPACE_SERVICE_KEY.api_version,
                    name=self.workspace,
                    required_features=frozenset(
                        (
                            WorkspaceOperation.FILES.value,
                            WorkspaceOperation.OBSERVATIONS.value,
                            WorkspaceOperation.SEARCH.value,
                        )
                    ),
                ),
            ),
        )

    def bind(self, services: AgentServices) -> Self:
        super().bind(services)
        session = services.resolve(workspace_ref(self.workspace))
        bound = type(self)(workspace=self.workspace)
        object.__setattr__(
            bound,
            'contributions',
            CapabilityContributions(
                instructions=(SEARCH_TOOL_INSTRUCTIONS,),
                tools=(GlobTool(provider=session.search),),
                toolsets=(
                    SearchSourceToolset(
                        provider=session.search,
                        state=session.edit_mode,
                        observations=session.observations,
                    ),
                ),
            ),
        )
        return bound
