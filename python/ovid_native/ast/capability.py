from dataclasses import dataclass, field
from typing import Self

from ovid_core.capabilities.base import BaseCapability, CapabilityContributions
from ovid_core.services import AgentServiceRequirement, AgentServices

from ovid_native.ast.tools import AST_TOOL_INSTRUCTIONS, AstEditApplyTool, AstEditPreviewTool, AstSourceToolset
from ovid_native.workspace.operations import WORKSPACE_SERVICE_KEY, WorkspaceOperation, workspace_ref


@dataclass(frozen=True, slots=True, kw_only=True)
class AstCapability[Deps](BaseCapability[Deps]):
    workspace: str = 'default'
    id: str = field(default='native_ast', init=False)
    description: str = field(
        default='Syntax-aware source search and staged structural rewrites',
        init=False,
    )
    defer_loading: bool = field(default=True, init=False)
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
                            WorkspaceOperation.AST.value,
                            WorkspaceOperation.FILES.value,
                            WorkspaceOperation.OBSERVATIONS.value,
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
                instructions=(AST_TOOL_INSTRUCTIONS,),
                tools=(
                    AstEditPreviewTool(provider=session.ast),
                    AstEditApplyTool(provider=session.ast),
                ),
                toolsets=(
                    AstSourceToolset(
                        provider=session.ast,
                        state=session.edit_mode,
                        observations=session.observations,
                    ),
                ),
            ),
        )
        return bound
