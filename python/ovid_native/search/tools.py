from collections.abc import Sequence
from typing import Any

from ovid_core.runtime.context import RunContext
from ovid_core.tools.base import BaseTool, BaseToolset, ToolExecutionContext

from ovid_native.files.edit_modes import EditModeState
from ovid_native.search.models import (
    GlobRequest,
    GlobToolContent,
    GlobToolResult,
    GrepFileMatches,
    GrepRequest,
    GrepResult,
    GrepToolContent,
    GrepToolRequest,
    GrepToolResult,
)
from ovid_native.workspace.evidence import (
    EditableSourceGroup,
    WorkspaceEvidence,
    WorkspaceObservationRequest,
    WorkspaceSourceLineClaim,
    WorkspaceSourcePresenter,
    WorkspaceSourceSpanClaim,
    capture_source_presentation,
)
from ovid_native.workspace.models import WorkspaceSearchProvider
from ovid_native.workspace.observations import WorkspaceLineRange, WorkspaceObservationService


GLOB_DESCRIPTION = (
    'Find workspace files and directories by path or glob pattern. Results respect ignore rules by default and are '
    'bounded. Directories end with `/`. A timeout or limit marks the result incomplete, so an empty incomplete result '
    'does not prove absence.'
)
GREP_DESCRIPTION = (
    'Search workspace file contents using embedded ripgrep. Use regex for text and configuration searches; use AST '
    'tools when syntax shape matters and LSP when symbol identity matters. Results are grouped and paginated by file '
    'so one hot file cannot consume the response. Partial large-file coverage, limits, and timeouts are reported.'
)
SEARCH_TOOL_INSTRUCTIONS = (
    'Use glob to discover workspace paths and grep to search file content. Narrow broad grep searches with paths or '
    'glob results. Grep pages by matching files and limits matches within each file; use the returned next offset to '
    'continue. A timeout, scan limit, or partial large-file coverage does not prove absence. Use AST tools for '
    'syntax-shaped searches and LSP for symbol identity.'
)


class GlobTool[Deps](BaseTool[Deps, GlobRequest, GlobToolResult]):
    id = 'glob'
    description = GLOB_DESCRIPTION
    args_type = GlobRequest
    result_type = GlobToolResult
    timeout_seconds = 5.0
    defer_loading = False

    def __init__(self, *, provider: WorkspaceSearchProvider) -> None:
        self._provider = provider

    async def execute(
        self,
        context: ToolExecutionContext[Deps],
        arguments: GlobRequest,
    ) -> GlobToolResult:
        del context
        result = await self._provider.glob(arguments)
        content = GlobToolContent(result=result).model_dump(mode='json')
        return GlobToolResult(content=content)


class GrepTool[Deps](BaseTool[Deps, GrepToolRequest, GrepToolResult]):
    id = 'grep'
    description = GREP_DESCRIPTION
    args_type = GrepToolRequest
    result_type = GrepToolResult
    timeout_seconds = 30.0
    defer_loading = False

    def __init__(
        self,
        *,
        provider: WorkspaceSearchProvider,
        presenter: WorkspaceSourcePresenter | None = None,
    ) -> None:
        self._provider = provider
        self._presenter = presenter

    async def execute(
        self,
        context: ToolExecutionContext[Deps],
        arguments: GrepToolRequest,
    ) -> GrepToolResult:
        del context
        result = await self._provider.grep(_grep_request(arguments))
        if self._presenter is None:
            return GrepToolResult(content=GrepToolContent(result=result).model_dump(mode='json'))
        groups = await _grep_source_groups(result, self._presenter)
        if self._presenter.presentation.format == 'hashline':
            content = '\n\n'.join(group.render(self._presenter.presentation) for group in groups)
            return GrepToolResult(
                content=content,
                metadata=GrepToolContent(result=result).model_dump(mode='json'),
            )
        content = GrepToolContent(result=result).model_dump(mode='json')
        return GrepToolResult(content=content)


class SearchSourceToolset[Deps](BaseToolset[Deps]):
    id = 'native_search_source'

    def __init__(
        self,
        *,
        provider: WorkspaceSearchProvider,
        state: EditModeState,
        observations: WorkspaceObservationService,
    ) -> None:
        self._provider = provider
        self._state = state
        self._observations = observations

    async def for_step(self, context: RunContext[Deps]) -> BaseToolset[Deps]:
        del context
        selection = self._state.current
        presenter = WorkspaceSourcePresenter(
            observations=self._observations,
            presentation=capture_source_presentation(selection.mode, selection.generation),
        )
        return _BoundSearchSourceToolset(
            owner=self,
            tool=GrepTool(provider=self._provider, presenter=presenter),
        )

    async def get_tools(self, context: RunContext[Deps]) -> Sequence[BaseTool[Deps, Any, Any]]:
        return await (await self.for_step(context)).get_tools(context)


class _BoundSearchSourceToolset[Deps](BaseToolset[Deps]):
    id = 'native_search_source'

    def __init__(self, *, owner: SearchSourceToolset[Deps], tool: BaseTool[Deps, Any, Any]) -> None:
        self._owner = owner
        self._tool = tool

    async def for_step(self, context: RunContext[Deps]) -> BaseToolset[Deps]:
        return await self._owner.for_step(context)

    async def get_tools(self, context: RunContext[Deps]) -> Sequence[BaseTool[Deps, Any, Any]]:
        del context
        return (self._tool,)


def _grep_request(arguments: GrepToolRequest) -> GrepRequest:
    return GrepRequest(
        pattern=arguments.pattern,
        scan=arguments.scan,
        mode=arguments.mode,
        case_sensitive=arguments.case_sensitive,
        multiline=arguments.multiline,
        file_offset=arguments.file_offset,
        file_limit=arguments.file_limit,
        matches_per_file=arguments.matches_per_file,
        context_before=arguments.context_before,
        context_after=arguments.context_after,
        max_file_bytes=arguments.max_file_bytes,
        large_file_mode=arguments.large_file_mode,
        timeout_seconds=arguments.timeout_seconds,
    )


async def _grep_source_groups(
    result: GrepResult,
    presenter: WorkspaceSourcePresenter,
) -> tuple[EditableSourceGroup, ...]:
    return tuple([await _grep_source_group(file, presenter) for file in result.files])


async def _grep_source_group(
    file: GrepFileMatches,
    presenter: WorkspaceSourcePresenter,
) -> EditableSourceGroup:
    claimed: dict[int, str] = {}
    spans: list[WorkspaceSourceSpanClaim] = []
    for match in file.matches:
        for line in match.matched_lines:
            claimed[line.line_number] = line.text
        for line in (*match.context_before, *match.context_after):
            if not line.truncated:
                claimed[line.line_number] = line.text
        spans.append(
            WorkspaceSourceSpanClaim(
                start_line=match.range.start.line,
                start_byte=match.range.start.byte_offset,
                end_line=match.range.end.line,
                end_byte=match.range.end.byte_offset,
            )
        )
    lines = tuple(WorkspaceSourceLineClaim(line_number=line, text=text) for line, text in sorted(claimed.items()))
    evidence = WorkspaceEvidence(
        path=file.path,
        revision=None,
        lines=lines,
        visible_ranges=_line_ranges(tuple(claimed)),
        spans=tuple(spans),
    )
    return await presenter.observe(
        WorkspaceObservationRequest(
            evidence=evidence,
            purpose='grep',
            presentation=presenter.presentation,
        )
    )


def _line_ranges(lines: tuple[int, ...]) -> tuple[WorkspaceLineRange, ...]:
    ranges: list[WorkspaceLineRange] = []
    for line in sorted(lines):
        if ranges and line == ranges[-1].end + 1:
            ranges[-1] = WorkspaceLineRange(start=ranges[-1].start, end=line)
        else:
            ranges.append(WorkspaceLineRange(start=line, end=line))
    return tuple(ranges)
