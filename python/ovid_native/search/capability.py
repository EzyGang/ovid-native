from dataclasses import dataclass, field

from ovid_core.capabilities.base import CapabilityContributions

from ovid_native.search.tools import SEARCH_TOOL_INSTRUCTIONS, GlobTool, SearchSourceToolset
from ovid_native.workspace._capability import WorkspaceCapability
from ovid_native.workspace.models import WorkspaceSession
from ovid_native.workspace.operations import WorkspaceOperation


@dataclass(frozen=True, slots=True, kw_only=True)
class SearchCapability[Deps](WorkspaceCapability[Deps]):
    id: str = field(default='native_search', init=False)
    description: str = field(
        default='Fast workspace path discovery and bounded text search',
        init=False,
    )
    defer_loading: bool = field(default=False, init=False)

    def _required_operations(self) -> frozenset[WorkspaceOperation]:
        return frozenset((WorkspaceOperation.FILES, WorkspaceOperation.OBSERVATIONS, WorkspaceOperation.SEARCH))

    def _contribute(self, session: WorkspaceSession) -> CapabilityContributions[Deps]:
        return CapabilityContributions(
            instructions=(SEARCH_TOOL_INSTRUCTIONS,),
            tools=(GlobTool(provider=session.search),),
            toolsets=(
                SearchSourceToolset(
                    provider=session.search,
                    state=session.edit_mode,
                    observations=session.observations,
                ),
            ),
        )
