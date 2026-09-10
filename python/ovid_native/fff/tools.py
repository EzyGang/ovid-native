import asyncio
from collections.abc import Sequence
from typing import Any

from ovid_core.tools.base import BaseTool, ToolExecutionContext

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
from ovid_native.workspace._toolsets import WorkspaceResultTool, WorkspaceSourceTool, WorkspaceSourceToolset
from ovid_native.workspace.evidence import (
    EditableSourceGroup,
    WorkspaceSourcePresenter,
    WorkspaceSourceSpanClaim,
    source_line_ranges,
)
from ovid_native.workspace.models import WorkspaceFffProvider
from ovid_native.workspace.observations import WorkspaceObservationService, WorkspaceRenderedLine


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


class FffFindTool[Deps](WorkspaceResultTool[Deps, WorkspaceFffProvider, FffFindRequest, FffFindToolResult]):
    id = 'find_files'
    description = FIND_DESCRIPTION
    args_type = FffFindRequest
    result_type = FffFindToolResult
    timeout_seconds = 10.0
    defer_loading = True

    async def _content(self, arguments: FffFindRequest) -> FffFindToolContent:
        return FffFindToolContent(result=await self._provider.find(arguments))


class FffGrepTool[Deps](WorkspaceSourceTool[Deps, WorkspaceFffProvider, FffGrepRequest, FffGrepToolResult]):
    id = 'grep'
    description = GREP_DESCRIPTION
    args_type = FffGrepRequest
    result_type = FffGrepToolResult
    timeout_seconds = 10.0
    defer_loading = True

    async def execute(self, context: ToolExecutionContext[Deps], arguments: FffGrepRequest) -> FffGrepToolResult:
        del context
        result = await self._provider.grep(arguments)
        return await self._present_result(result, _fff_source_groups)


class FffMultiGrepTool[Deps](
    WorkspaceSourceTool[Deps, WorkspaceFffProvider, FffMultiGrepRequest, FffMultiGrepToolResult]
):
    id = 'multi_grep'
    description = MULTI_GREP_DESCRIPTION
    args_type = FffMultiGrepRequest
    result_type = FffMultiGrepToolResult
    timeout_seconds = 10.0
    defer_loading = True

    async def execute(
        self,
        context: ToolExecutionContext[Deps],
        arguments: FffMultiGrepRequest,
    ) -> FffMultiGrepToolResult:
        del context
        result = await self._provider.multi_grep(arguments)
        return await self._present_result(result, _fff_source_groups)


class FffSourceToolset[Deps](WorkspaceSourceToolset[Deps, WorkspaceFffProvider]):
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
        super().__init__(provider=provider, state=state, observations=observations)
        self._include_grep = include_grep
        self._include_multi_grep = include_multi_grep

    def _source_tools(self, presenter: WorkspaceSourcePresenter) -> Sequence[BaseTool[Deps, Any, Any]]:
        tools: list[BaseTool[Deps, Any, Any]] = []
        if self._include_grep:
            tools.append(FffGrepTool(provider=self._provider, presenter=presenter))
        if self._include_multi_grep:
            tools.append(FffMultiGrepTool(provider=self._provider, presenter=presenter))
        return tools


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
    reason = None
    if any(match.approximate for match in matches):
        reason = 'FFF match is approximate; rerun an exact source-producing tool'
    elif any(not match.match_ranges for match in matches):
        reason = 'FFF match has no exact source span; rerun an exact source-producing tool'
    if reason is not None:
        return EditableSourceGroup(
            path=path,
            observation=None,
            editable=False,
            lines=tuple(
                WorkspaceRenderedLine(line_number=line, short_hash='--', text=text)
                for line, text in sorted(claimed.items())
            ),
            visible_ranges=source_line_ranges(claimed),
            uneditable_reason=reason,
        )
    return await presenter.observe_lines(
        path=path,
        revision=revision,
        lines=claimed,
        spans=_fff_spans(matches),
        purpose='fff_grep',
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
