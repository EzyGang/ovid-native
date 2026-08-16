from ovid_core.tools.base import BaseTool, ToolExecutionContext

from ovid_native.fff.models import (
    FffFindRequest,
    FffFindToolContent,
    FffFindToolResult,
    FffGrepRequest,
    FffGrepToolContent,
    FffGrepToolResult,
    FffMultiGrepRequest,
    FffMultiGrepToolContent,
    FffMultiGrepToolResult,
)
from ovid_native.workspace.models import WorkspaceFffProvider


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

    def __init__(self, *, provider: WorkspaceFffProvider) -> None:
        self._provider = provider

    async def execute(self, context: ToolExecutionContext[Deps], arguments: FffGrepRequest) -> FffGrepToolResult:
        del context
        result = await self._provider.grep(arguments)
        return FffGrepToolResult(content=FffGrepToolContent(result=result).model_dump(mode='json'))


class FffMultiGrepTool[Deps](BaseTool[Deps, FffMultiGrepRequest, FffMultiGrepToolResult]):
    id = 'multi_grep'
    description = MULTI_GREP_DESCRIPTION
    args_type = FffMultiGrepRequest
    result_type = FffMultiGrepToolResult
    timeout_seconds = 10.0
    defer_loading = True

    def __init__(self, *, provider: WorkspaceFffProvider) -> None:
        self._provider = provider

    async def execute(
        self,
        context: ToolExecutionContext[Deps],
        arguments: FffMultiGrepRequest,
    ) -> FffMultiGrepToolResult:
        del context
        result = await self._provider.multi_grep(arguments)
        return FffMultiGrepToolResult(content=FffMultiGrepToolContent(result=result).model_dump(mode='json'))
