import asyncio
from collections.abc import Sequence
from typing import Any

from ovid_core.runtime.context import RunContext
from ovid_core.tools.base import BaseTool, BaseToolset, ToolExecutionContext

from ovid_native.fff.models import (
    FffFindRequest,
    FffFindToolContent,
    FffFindToolResult,
    FffGrepMatch,
    FffGrepRequest,
    FffGrepResult,
    FffGrepToolResult,
    FffMultiGrepRequest,
    FffMultiGrepToolResult,
)
from ovid_native.files.edit_modes import EditModeState
from ovid_native.workspace.evidence import (
    EditableSourceGroup,
    WorkspaceEvidence,
    WorkspaceObservationRequest,
    WorkspaceSourceLineClaim,
    WorkspaceSourcePresenter,
    WorkspaceSourceSpanClaim,
    capture_source_presentation,
)
from ovid_native.workspace.models import WorkspaceFffProvider
from ovid_native.workspace.observations import WorkspaceLineRange, WorkspaceObservationService, WorkspaceRenderedLine


FIND_DESCRIPTION = (
    'Find indexed files or directories by approximate path. FFF tolerates typos and ranks likely paths. Use short '
    'one- or two-term queries. When available, use glob for exact path patterns. FFF searches its indexed universe, '
    'so an empty result does not prove absence.'
)
GREP_DESCRIPTION = (
    'Search indexed file content using plain, regex, fuzzy, or auto matching. Use fuzzy or auto when spelling or '
    'naming may vary. This grep backend searches the FFF indexed universe and paginates by file offset.'
)
MULTI_GREP_DESCRIPTION = (
    'Search indexed content for lines matching any of several literal patterns. Use for snake_case, PascalCase, '
    'camelCase, aliases, or related identifiers in one call. Patterns use OR semantics and are never regexes.'
)


class FffFindTool[Deps](BaseTool[Deps, FffFindRequest, FffFindToolResult]):
    id = 'find_files'
    description = FIND_DESCRIPTION
    args_type = FffFindRequest
    result_type = FffFindToolResult
    timeout_seconds = 10.0
    defer_loading = True

    def __init__(self, *, provider: WorkspaceFffProvider) -> None:
        self._provider = provider

    async def execute(self, context: ToolExecutionContext[Deps], arguments: FffFindRequest) -> FffFindToolResult:
        del context
        result = await self._provider.find(arguments)
        return FffFindToolResult(content=FffFindToolContent(result=result).model_dump(mode='json'))


class FffGrepTool[Deps](BaseTool[Deps, FffGrepRequest, FffGrepToolResult]):
    id = 'grep'
    description = GREP_DESCRIPTION
    args_type = FffGrepRequest
    result_type = FffGrepToolResult
    timeout_seconds = 10.0
    defer_loading = True

    def __init__(
        self,
        *,
        provider: WorkspaceFffProvider,
        presenter: WorkspaceSourcePresenter | None = None,
    ) -> None:
        self._provider = provider
        self._presenter = presenter

    async def execute(self, context: ToolExecutionContext[Deps], arguments: FffGrepRequest) -> FffGrepToolResult:
        del context
        result = await self._provider.grep(arguments)
        return await _grep_result(result, self._presenter, result_type=FffGrepToolResult)


class FffMultiGrepTool[Deps](BaseTool[Deps, FffMultiGrepRequest, FffMultiGrepToolResult]):
    id = 'multi_grep'
    description = MULTI_GREP_DESCRIPTION
    args_type = FffMultiGrepRequest
    result_type = FffMultiGrepToolResult
    timeout_seconds = 10.0
    defer_loading = True

    def __init__(
        self,
        *,
        provider: WorkspaceFffProvider,
        presenter: WorkspaceSourcePresenter | None = None,
    ) -> None:
        self._provider = provider
        self._presenter = presenter

    async def execute(
        self,
        context: ToolExecutionContext[Deps],
        arguments: FffMultiGrepRequest,
    ) -> FffMultiGrepToolResult:
        del context
        result = await self._provider.multi_grep(arguments)
        return await _grep_result(result, self._presenter, result_type=FffMultiGrepToolResult)


class FffSourceToolset[Deps](BaseToolset[Deps]):
    id = 'native_fff_source'

    def __init__(
        self,
        *,
        provider: WorkspaceFffProvider,
        state: EditModeState,
        observations: WorkspaceObservationService,
        include_grep: bool,
        include_multi_grep: bool,
    ) -> None:
        self._provider = provider
        self._state = state
        self._observations = observations
        self._include_grep = include_grep
        self._include_multi_grep = include_multi_grep

    async def for_step(self, context: RunContext[Deps]) -> BaseToolset[Deps]:
        del context
        selection = self._state.current
        presenter = WorkspaceSourcePresenter(
            observations=self._observations,
            presentation=capture_source_presentation(selection.mode, selection.generation),
        )
        tools: list[BaseTool[Deps, Any, Any]] = []
        if self._include_grep:
            tools.append(FffGrepTool(provider=self._provider, presenter=presenter))
        if self._include_multi_grep:
            tools.append(FffMultiGrepTool(provider=self._provider, presenter=presenter))
        return _BoundFffSourceToolset(owner=self, tools=tools)

    async def get_tools(self, context: RunContext[Deps]) -> Sequence[BaseTool[Deps, Any, Any]]:
        return await (await self.for_step(context)).get_tools(context)


class _BoundFffSourceToolset[Deps](BaseToolset[Deps]):
    id = 'native_fff_source'

    def __init__(
        self,
        *,
        owner: FffSourceToolset[Deps],
        tools: Sequence[BaseTool[Deps, Any, Any]],
    ) -> None:
        self._owner = owner
        self._tools = tuple(tools)

    async def for_step(self, context: RunContext[Deps]) -> BaseToolset[Deps]:
        return await self._owner.for_step(context)

    async def get_tools(self, context: RunContext[Deps]) -> Sequence[BaseTool[Deps, Any, Any]]:
        del context
        return self._tools


async def _grep_result[Result: (FffGrepToolResult, FffMultiGrepToolResult)](
    result: FffGrepResult,
    presenter: WorkspaceSourcePresenter | None,
    *,
    result_type: type[Result],
) -> Result:
    metadata = {'result': result.model_dump(mode='json')}
    if presenter is None:
        return result_type(content=metadata)
    groups = await _fff_source_groups(result, presenter)
    if presenter.presentation.format == 'hashline':
        content = '\n\n'.join(group.render(presenter.presentation) for group in groups)
        return result_type(content=content, metadata=metadata)
    return result_type(content=metadata)


async def _fff_source_groups(
    result: FffGrepResult,
    presenter: WorkspaceSourcePresenter,
) -> tuple[EditableSourceGroup, ...]:
    grouped: dict[str, list[FffGrepMatch]] = {}
    for match in result.matches:
        grouped.setdefault(match.path, []).append(match)
    return tuple(
        await asyncio.gather(
            *(
                _observe_fff_path(
                    path,
                    matches,
                    presenter,
                    revision=result.workspace_revision,
                )
                for path, matches in grouped.items()
            )
        )
    )


async def _observe_fff_path(
    path: str,
    matches: list[FffGrepMatch],
    presenter: WorkspaceSourcePresenter,
    *,
    revision: str | None,
) -> EditableSourceGroup:
    claimed = {line_number: text for match in matches for line_number, text in _fff_lines(match)}
    ranges = _line_ranges(tuple(claimed))
    if any(match.approximate for match in matches):
        return EditableSourceGroup(
            path=path,
            observation=None,
            editable=False,
            lines=tuple(
                WorkspaceRenderedLine(line_number=line, short_hash='--', text=text)
                for line, text in sorted(claimed.items())
            ),
            visible_ranges=ranges,
            uneditable_reason='FFF match is approximate; rerun an exact source-producing tool',
        )
    evidence = WorkspaceEvidence(
        path=path,
        revision=revision,
        lines=tuple(WorkspaceSourceLineClaim(line_number=line, text=text) for line, text in sorted(claimed.items())),
        visible_ranges=ranges,
        spans=_fff_spans(matches),
    )
    return await presenter.observe(
        WorkspaceObservationRequest(
            evidence=evidence,
            purpose='fff_grep',
            presentation=presenter.presentation,
        )
    )


def _fff_lines(match: FffGrepMatch) -> tuple[tuple[int, str], ...]:
    return (
        *((line.line_number, line.text) for line in match.context_before),
        (match.line_number, match.line),
        *((line.line_number, line.text) for line in match.context_after),
    )


def _fff_spans(matches: list[FffGrepMatch]) -> tuple[WorkspaceSourceSpanClaim, ...]:
    spans: list[WorkspaceSourceSpanClaim] = []
    for match in matches:
        if not match.match_ranges:
            continue
        line_start = match.byte_offset - match.match_ranges[0].start
        spans.extend(
            WorkspaceSourceSpanClaim(
                start_line=match.line_number,
                start_byte=line_start + byte_range.start,
                end_line=match.line_number,
                end_byte=line_start + byte_range.end,
            )
            for byte_range in match.match_ranges
        )
    return tuple(spans)


def _line_ranges(lines: tuple[int, ...]) -> tuple[WorkspaceLineRange, ...]:
    ranges: list[WorkspaceLineRange] = []
    for line in sorted(lines):
        if ranges and line == ranges[-1].end + 1:
            ranges[-1] = WorkspaceLineRange(start=ranges[-1].start, end=line)
        else:
            ranges.append(WorkspaceLineRange(start=line, end=line))
    return tuple(ranges)
