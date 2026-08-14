from dataclasses import dataclass, field
from typing import Any

from ovid_core.capabilities.base import BaseCapability, CapabilityContributions
from ovid_core.services import AgentServiceRequirement, AgentServices
from ovid_core.tools.base import BaseTool

from ovid_native.fff.errors import FffConfigurationError
from ovid_native.fff.tools import FffFindTool, FffGrepTool, FffMultiGrepTool
from ovid_native.search.tools import GlobTool
from ovid_native.workspace.models import WorkspaceOperation
from ovid_native.workspace.service import _workspace_requirement, workspace_ref


@dataclass(frozen=True, slots=True, kw_only=True)
class _BoundFffCapability[Deps](BaseCapability[Deps]):
    pass


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
    requirements: tuple[AgentServiceRequirement, ...] = field(init=False)
    contributions: CapabilityContributions[Deps] = field(default=CapabilityContributions(), init=False, repr=False)

    def __post_init__(self) -> None:
        if not any((self.include_glob, self.include_find_files, self.include_grep, self.include_multi_grep)):
            raise FffConfigurationError('at least one FFF tool must be enabled')

        operations = [WorkspaceOperation.FFF]
        if self.include_glob:
            operations.append(WorkspaceOperation.SEARCH)
        object.__setattr__(
            self,
            'requirements',
            tuple(_workspace_requirement(operation, name=self.workspace) for operation in operations),
        )

    def bind(self, services: AgentServices) -> _BoundFffCapability[Deps]:
        super().bind(services)
        session = services.resolve(workspace_ref(self.workspace))
        tools: list[BaseTool[Deps, Any, Any]] = []
        if self.include_glob:
            tools.append(GlobTool(provider=session.search))
        if self.include_find_files:
            tools.append(FffFindTool(provider=session.fff))
        if self.include_grep:
            tools.append(FffGrepTool(provider=session.fff))
        if self.include_multi_grep:
            tools.append(FffMultiGrepTool(provider=session.fff))

        return _BoundFffCapability(
            id=self.id,
            description=self.description,
            defer_loading=self.defer_loading,
            requirements=self.requirements,
            contributions=CapabilityContributions(
                instructions=(self._instructions(),),
                tools=tuple(tools),
            ),
        )

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
