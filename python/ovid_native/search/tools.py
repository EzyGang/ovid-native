from collections.abc import Sequence
from typing import Any

from ovid_core.tools.base import BaseTool, ToolExecutionContext

from ovid_native.search.models import (
    GlobRequest,
    GlobToolContent,
    GlobToolResult,
    GrepFileMatches,
    GrepRequest,
    GrepResult,
    GrepToolRequest,
    GrepToolResult,
)
from ovid_native.workspace._toolsets import WorkspaceResultTool, WorkspaceSourceTool, WorkspaceSourceToolset
from ovid_native.workspace.evidence import (
    EditableSourceGroup,
    WorkspaceSourcePresenter,
    WorkspaceSourceSpanClaim,
    normalize_terminal_span_end,
)
from ovid_native.workspace.models import WorkspaceSearchProvider


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


class GlobTool[Deps](WorkspaceResultTool[Deps, WorkspaceSearchProvider, GlobRequest, GlobToolResult]):
    id = 'glob'
    description = GLOB_DESCRIPTION
    args_type = GlobRequest
    result_type = GlobToolResult
    timeout_seconds = 5.0
    defer_loading = False

    async def _content(self, arguments: GlobRequest) -> GlobToolContent:
        return GlobToolContent(result=await self._provider.glob(arguments))


class GrepTool[Deps](WorkspaceSourceTool[Deps, WorkspaceSearchProvider, GrepToolRequest, GrepToolResult]):
    id = 'grep'
    description = GREP_DESCRIPTION
    args_type = GrepToolRequest
    result_type = GrepToolResult
    timeout_seconds = 30.0
    defer_loading = False

    async def execute(
        self,
        context: ToolExecutionContext[Deps],
        arguments: GrepToolRequest,
    ) -> GrepToolResult:
        del context
        result = await self._provider.grep(_grep_request(arguments))
        return await self._present_result(result, _grep_source_groups)


class SearchSourceToolset[Deps](WorkspaceSourceToolset[Deps, WorkspaceSearchProvider]):
    id = 'native_search_source'

    def _source_tools(self, presenter: WorkspaceSourcePresenter) -> Sequence[BaseTool[Deps, Any, Any]]:
        return (GrepTool(provider=self._provider, presenter=presenter),)


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
    for match in file.matches:
        for line in match.matched_lines:
            claimed[line.line_number] = line.text
        for line in (*match.context_before, *match.context_after):
            if not line.truncated:
                claimed[line.line_number] = line.text

    spans = tuple(
        WorkspaceSourceSpanClaim(
            start_line=match.range.start.line,
            start_byte=match.range.start.byte_offset,
            end_line=end_line,
            end_byte=end_byte,
        )
        for match in file.matches
        for end_line, end_byte in (
            normalize_terminal_span_end(
                end_line=match.range.end.line,
                end_column=match.range.end.column,
                end_byte=match.range.end.byte_offset,
                claimed_lines=claimed,
            ),
        )
    )
    return await presenter.observe_lines(path=file.path, lines=claimed, spans=spans, purpose='grep')
