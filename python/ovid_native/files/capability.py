from dataclasses import dataclass, field
from typing import Self, cast

from ovid_core.capabilities.base import BaseCapability, CapabilityContributions
from ovid_core.services import AgentServiceRequirement, AgentServices

from ovid_native.files.engine import WorkspaceFilesEngine
from ovid_native.files.tools import FILES_TOOL_INSTRUCTIONS, EditModeToolset, ReadTool, WriteTool
from ovid_native.workspace.operations import WORKSPACE_SERVICE_KEY, WorkspaceOperation, workspace_ref


@dataclass(frozen=True, slots=True, kw_only=True)
class WorkspaceFilesCapability[Deps](BaseCapability[Deps]):
    workspace: str = 'default'
    id: str = field(default='native_files', init=False)
    description: str = field(
        default='Workspace text reading and mode-aware file mutation',
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
                            WorkspaceOperation.CHANGE_EVENTS.value,
                        )
                    ),
                ),
            ),
        )

    def bind(self, services: AgentServices) -> Self:
        super().bind(services)
        session = services.resolve(workspace_ref(self.workspace))
        provider = cast(WorkspaceFilesEngine, session.files)
        bound = type(self)(workspace=self.workspace)
        object.__setattr__(
            bound,
            'contributions',
            CapabilityContributions(
                instructions=(FILES_TOOL_INSTRUCTIONS,),
                tools=(ReadTool(provider=provider), WriteTool(provider=provider)),
                toolsets=(EditModeToolset(provider=provider, state=session.edit_mode),),
            ),
        )
        return bound
