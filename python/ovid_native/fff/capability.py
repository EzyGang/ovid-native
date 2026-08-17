from dataclasses import dataclass, field
from typing import Any, Self

from ovid_core.capabilities.base import BaseCapability, CapabilityContributions
from ovid_core.services import AgentServiceRequirement, AgentServices
from ovid_core.tools.base import BaseTool

from ovid_native.fff.errors import FffConfigurationError
from ovid_native.fff.tools import FffFindTool, FffSourceToolset
from ovid_native.search.tools import GlobTool
from ovid_native.workspace.operations import WORKSPACE_SERVICE_KEY, WorkspaceOperation, workspace_ref


@dataclass(frozen=True, slots=True, kw_only=True)
class FffCapability[Deps](BaseCapability[Deps]):
    workspace: str = 'default'
    include_glob: bool = False
    include_find_files: bool = True
    include_grep: bool = True
    include_multi_grep: bool = True
    id: str = field(default='native_fff', init=False)
    description: str = field(default='Warm typo-resistant path and content search', init=False)
    defer_loading: bool = field(default=True, init=False)
    contributions: CapabilityContributions[Deps] = field(init=False, repr=False)
    requirements: tuple[AgentServiceRequirement, ...] = field(init=False)

    def __post_init__(self) -> None:
        if not any((self.include_glob, self.include_find_files, self.include_grep, self.include_multi_grep)):
            raise FffConfigurationError('at least one FFF tool must be enabled')

        required_features: set[str] = set()
        if any((self.include_find_files, self.include_grep, self.include_multi_grep)):
            required_features.add(WorkspaceOperation.FFF.value)
        if self.include_glob:
            required_features.add(WorkspaceOperation.SEARCH.value)
        if self.include_grep or self.include_multi_grep:
            required_features.update((WorkspaceOperation.FILES.value, WorkspaceOperation.OBSERVATIONS.value))

        object.__setattr__(self, 'contributions', CapabilityContributions())
        object.__setattr__(
            self,
            'requirements',
            (
                AgentServiceRequirement(
                    service_id=WORKSPACE_SERVICE_KEY.id,
                    api_version=WORKSPACE_SERVICE_KEY.api_version,
                    name=self.workspace,
                    required_features=frozenset(required_features),
                ),
            ),
        )

    def bind(self, services: AgentServices) -> Self:
        super().bind(services)
        session = services.resolve(workspace_ref(self.workspace))
        tools: list[BaseTool[Deps, Any, Any]] = []

        if self.include_glob:
            tools.append(GlobTool(provider=session.search))
        if self.include_find_files:
            tools.append(FffFindTool(provider=session.fff))

        bound = type(self)(
            workspace=self.workspace,
            include_glob=self.include_glob,
            include_find_files=self.include_find_files,
            include_grep=self.include_grep,
            include_multi_grep=self.include_multi_grep,
        )
        object.__setattr__(
            bound,
            'contributions',
            CapabilityContributions(
                instructions=(self._instructions(),),
                tools=tuple(tools),
                toolsets=(
                    FffSourceToolset(
                        provider=session.fff,
                        state=session.edit_mode,
                        observations=session.observations,
                        include_grep=self.include_grep,
                        include_multi_grep=self.include_multi_grep,
                    ),
                )
                if self.include_grep or self.include_multi_grep
                else (),
            ),
        )
        return bound

    def _instructions(self) -> str:
        instructions: list[str] = []
        if self.include_glob:
            instructions.append('Use glob for exact path discovery.')
        if self.include_find_files:
            instructions.append('Use find_files for approximate filenames or paths. Keep queries to one or two terms.')
        if self.include_grep:
            instructions.append(
                'Use grep for typo-tolerant or repeated indexed content search. Continue with next_file_offset when '
                'present.'
            )
        if self.include_multi_grep:
            instructions.append('Use multi_grep for literal OR searches across naming variants.')
        instructions.append(
            'FFF searches a warm indexed subset and may exclude ignored, binary, or oversized files; empty FFF results '
            'do not prove absence. When available, use AST tools for syntax structure and LSP for symbol identity.'
        )
        return ' '.join(instructions)
