from dataclasses import dataclass, field
from typing import Any

from ovid_core.capabilities.base import CapabilityContributions
from ovid_core.tools.base import BaseTool

from ovid_native.fff.errors import FffConfigurationError
from ovid_native.fff.tools import FffFindTool, FffSourceToolset
from ovid_native.search.tools import GlobTool
from ovid_native.workspace._capability import WorkspaceCapability
from ovid_native.workspace.models import WorkspaceSession
from ovid_native.workspace.operations import WorkspaceOperation


@dataclass(frozen=True, slots=True, kw_only=True)
class FffCapability[Deps](WorkspaceCapability[Deps]):
    include_glob: bool = False
    include_find_files: bool = True
    include_grep: bool = True
    include_multi_grep: bool = True
    id: str = field(default='native_fff', init=False)
    description: str = field(default='Warm typo-resistant path and content search', init=False)
    defer_loading: bool = field(default=True, init=False)

    def _required_operations(self) -> frozenset[WorkspaceOperation]:
        if not any((self.include_glob, self.include_find_files, self.include_grep, self.include_multi_grep)):
            raise FffConfigurationError('at least one FFF tool must be enabled')

        operations: set[WorkspaceOperation] = set()
        if any((self.include_find_files, self.include_grep, self.include_multi_grep)):
            operations.add(WorkspaceOperation.FFF)
        if self.include_glob:
            operations.add(WorkspaceOperation.SEARCH)
        if self.include_grep or self.include_multi_grep:
            operations.update((WorkspaceOperation.FILES, WorkspaceOperation.OBSERVATIONS))
        return frozenset(operations)

    def _contribute(self, session: WorkspaceSession) -> CapabilityContributions[Deps]:
        tools: list[BaseTool[Deps, Any, Any]] = []

        if self.include_glob:
            tools.append(GlobTool(provider=session.search))
        if self.include_find_files:
            tools.append(FffFindTool(provider=session.fff))

        return CapabilityContributions(
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
