from dataclasses import dataclass, field

from ovid_core.capabilities.base import BaseCapability, CapabilityContributions

from ovid_native.search.engine import SearchEngine
from ovid_native.search.tools import SEARCH_TOOL_INSTRUCTIONS, GlobTool, GrepTool


@dataclass(frozen=True, slots=True, kw_only=True)
class SearchCapability[Deps](BaseCapability[Deps]):
    engine: SearchEngine
    id: str = field(default='native_search', init=False)
    description: str = field(
        default='Fast workspace path discovery and bounded text search',
        init=False,
    )
    defer_loading: bool = field(default=False, init=False)
    contributions: CapabilityContributions[Deps] = field(init=False, repr=False)

    def __post_init__(self) -> None:
        object.__setattr__(
            self,
            'contributions',
            CapabilityContributions(
                instructions=(SEARCH_TOOL_INSTRUCTIONS,),
                tools=(
                    GlobTool(engine=self.engine),
                    GrepTool(engine=self.engine),
                ),
            ),
        )
