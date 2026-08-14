from dataclasses import dataclass, field

from ovid_core.capabilities.base import BaseCapability, CapabilityContributions
from ovid_core.services import AgentServiceRequirement, AgentServices

from ovid_native.search.tools import SEARCH_TOOL_INSTRUCTIONS, GlobTool, GrepTool
from ovid_native.workspace.models import WorkspaceOperation
from ovid_native.workspace.service import _workspace_requirement, workspace_ref


@dataclass(frozen=True, slots=True, kw_only=True)
class _BoundSearchCapability[Deps](BaseCapability[Deps]):
    pass


@dataclass(frozen=True, slots=True, kw_only=True)
class SearchCapability[Deps](BaseCapability[Deps]):
    workspace: str = 'default'
    id: str = field(default='native_search', init=False)
    description: str = field(
        default='Fast workspace path discovery and bounded text search',
        init=False,
    )
    defer_loading: bool = field(default=False, init=False)
    requirements: tuple[AgentServiceRequirement, ...] = field(init=False)
    contributions: CapabilityContributions[Deps] = field(default=CapabilityContributions(), init=False, repr=False)

    def __post_init__(self) -> None:
        object.__setattr__(
            self,
            'requirements',
            (_workspace_requirement(WorkspaceOperation.SEARCH, name=self.workspace),),
        )

    def bind(self, services: AgentServices) -> _BoundSearchCapability[Deps]:
        super().bind(services)
        provider = services.resolve(workspace_ref(self.workspace)).search

        return _BoundSearchCapability(
            id=self.id,
            description=self.description,
            defer_loading=self.defer_loading,
            requirements=self.requirements,
            contributions=CapabilityContributions(
                instructions=(SEARCH_TOOL_INSTRUCTIONS,),
                tools=(GlobTool(provider=provider), GrepTool(provider=provider)),
            ),
        )
