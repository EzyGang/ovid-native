from ovid_core.tools.base import BaseTool, ToolExecutionContext

from ovid_native.search.models import (
    GlobRequest,
    GlobToolContent,
    GlobToolResult,
    GrepRequest,
    GrepToolContent,
    GrepToolRequest,
    GrepToolResult,
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

    def __init__(self, *, provider: WorkspaceSearchProvider) -> None:
        self._provider = provider

    async def execute(
        self,
        context: ToolExecutionContext[Deps],
        arguments: GrepToolRequest,
    ) -> GrepToolResult:
        del context
        result = await self._provider.grep(_grep_request(arguments))
        content = GrepToolContent(result=result).model_dump(mode='json')
        return GrepToolResult(content=content)


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
