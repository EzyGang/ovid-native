from dataclasses import dataclass, field

from ovid_core.capabilities.base import BaseCapability, CapabilityContributions

from ovid_native.ast.engine import AstEngine
from ovid_native.ast.tools import AST_TOOL_INSTRUCTIONS, AstEditApplyTool, AstEditPreviewTool, AstGrepTool


@dataclass(frozen=True, slots=True, kw_only=True)
class AstCapability[Deps](BaseCapability[Deps]):
    engine: AstEngine
    id: str = field(default='native_ast', init=False)
    description: str = field(
        default='Syntax-aware source search and staged structural rewrites',
        init=False,
    )
    defer_loading: bool = field(default=True, init=False)
    contributions: CapabilityContributions[Deps] = field(init=False, repr=False)

    def __post_init__(self) -> None:
        object.__setattr__(
            self,
            'contributions',
            CapabilityContributions(
                instructions=(AST_TOOL_INSTRUCTIONS,),
                tools=(
                    AstGrepTool(engine=self.engine),
                    AstEditPreviewTool(engine=self.engine),
                    AstEditApplyTool(engine=self.engine),
                ),
            ),
        )
