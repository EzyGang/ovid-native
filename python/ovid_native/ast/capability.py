from dataclasses import dataclass, field

from ovid_core.capabilities.base import BaseCapability, CapabilityContributions
from ovid_core.services import AgentServiceRequirement, AgentServices

from ovid_native.ast.tools import AST_TOOL_INSTRUCTIONS, AstEditApplyTool, AstEditPreviewTool, AstGrepTool
from ovid_native.workspace.models import WorkspaceOperation
from ovid_native.workspace.service import _workspace_requirement, workspace_ref


@dataclass(frozen=True, slots=True, kw_only=True)
class _BoundAstCapability[Deps](BaseCapability[Deps]):
    pass


@dataclass(frozen=True, slots=True, kw_only=True)
class AstCapability[Deps](BaseCapability[Deps]):
    workspace: str = 'default'
    id: str = field(default='native_ast', init=False)
    description: str = field(
        default='Syntax-aware source search and staged structural rewrites',
        init=False,
    )
    defer_loading: bool = field(default=True, init=False)
    requirements: tuple[AgentServiceRequirement, ...] = field(init=False)
    contributions: CapabilityContributions[Deps] = field(default=CapabilityContributions(), init=False, repr=False)

    def __post_init__(self) -> None:
        object.__setattr__(
            self,
            'requirements',
            (_workspace_requirement(WorkspaceOperation.AST, name=self.workspace),),
        )

    def bind(self, services: AgentServices) -> _BoundAstCapability[Deps]:
        super().bind(services)
        provider = services.resolve(workspace_ref(self.workspace)).ast

        return _BoundAstCapability(
            id=self.id,
            description=self.description,
            defer_loading=self.defer_loading,
            requirements=self.requirements,
            contributions=CapabilityContributions(
                instructions=(AST_TOOL_INSTRUCTIONS,),
                tools=(
                    AstGrepTool(provider=provider),
                    AstEditPreviewTool(provider=provider),
                    AstEditApplyTool(provider=provider),
                ),
            ),
        )
