from dataclasses import dataclass, field

from ovid_core.capabilities.base import CapabilityContributions

from ovid_native.ast.tools import AST_TOOL_INSTRUCTIONS, AstEditApplyTool, AstEditPreviewTool, AstSourceToolset
from ovid_native.workspace._capability import WorkspaceCapability
from ovid_native.workspace.models import WorkspaceSession
from ovid_native.workspace.operations import WorkspaceOperation


@dataclass(frozen=True, slots=True, kw_only=True)
class AstCapability[Deps](WorkspaceCapability[Deps]):
    id: str = field(default='native_ast', init=False)
    description: str = field(
        default='Syntax-aware source search and staged structural rewrites',
        init=False,
    )
    defer_loading: bool = field(default=True, init=False)

    def _required_operations(self) -> frozenset[WorkspaceOperation]:
        return frozenset((WorkspaceOperation.AST, WorkspaceOperation.FILES, WorkspaceOperation.OBSERVATIONS))

    def _contribute(self, session: WorkspaceSession) -> CapabilityContributions[Deps]:
        return CapabilityContributions(
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
        )
