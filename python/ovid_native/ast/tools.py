from ovid_core.tools.base import BaseTool, ToolExecutionContext
from ovid_core.tools.models import ToolApproval

from ovid_native.ast.models import (
    AstRewriteApplyRequest,
    AstRewriteApplyToolContent,
    AstRewriteApplyToolResult,
    AstRewritePreviewRequest,
    AstRewritePreviewToolContent,
    AstRewritePreviewToolResult,
    AstSearchRequest,
    AstSearchToolContent,
    AstSearchToolResult,
)
from ovid_native.workspace.operations import WorkspaceAstProvider


AST_GREP_DESCRIPTION = (
    'Search source code by syntax structure using ast-grep. Use when call, declaration, import, or expression shape '
    'matters. `$NAME` captures one node, `$_` matches one node, and `$$$ARGS` captures zero or more nodes. Parse '
    'issues mean the query was not evaluated for those files.'
)
AST_EDIT_PREVIEW_DESCRIPTION = (
    'Preview syntax-aware rewrites. Patterns match AST structure and captured metavariables are substituted into '
    'replacements. The tool writes nothing and returns a proposal ID for explicit application. Use ordinary edit for '
    'isolated changes and LSP rename for symbol-aware renames.'
)
AST_EDIT_APPLY_DESCRIPTION = (
    'Apply one previously previewed AST rewrite. Requires the proposal ID returned by `ast_edit_preview`. Application '
    'fails if the proposal expired or any affected file changed after preview.'
)
AST_TOOL_INSTRUCTIONS = (
    'Use ast_grep when syntax shape matters more than text. Patterns must parse in the target language. $NAME captures '
    'one node, $_ matches one node without binding, and $$$ARGS captures zero or more nodes. Reusing the same '
    'metavariable requires the same syntax in every position. Use ast_edit_preview for repeated structural changes, '
    'inspect its proposed changes, then use ast_edit_apply with the returned proposal ID. Use LSP for symbol identity '
    'and ordinary edit for isolated changes. Parse issues are failures for those files, not evidence of no match.'
)


class AstGrepTool[Deps](BaseTool[Deps, AstSearchRequest, AstSearchToolResult]):
    id = 'ast_grep'
    description = AST_GREP_DESCRIPTION
    args_type = AstSearchRequest
    result_type = AstSearchToolResult
    timeout_seconds = 30.0
    defer_loading = True

    def __init__(self, *, provider: WorkspaceAstProvider) -> None:
        self._provider = provider

    async def execute(
        self,
        context: ToolExecutionContext[Deps],
        arguments: AstSearchRequest,
    ) -> AstSearchToolResult:
        del context
        result = await self._provider.search(arguments)
        content = AstSearchToolContent(result=result).model_dump(mode='json')
        return AstSearchToolResult(content=content)


class AstEditPreviewTool[Deps](BaseTool[Deps, AstRewritePreviewRequest, AstRewritePreviewToolResult]):
    id = 'ast_edit_preview'
    description = AST_EDIT_PREVIEW_DESCRIPTION
    args_type = AstRewritePreviewRequest
    result_type = AstRewritePreviewToolResult
    timeout_seconds = 30.0
    defer_loading = True

    def __init__(self, *, provider: WorkspaceAstProvider) -> None:
        self._provider = provider

    async def execute(
        self,
        context: ToolExecutionContext[Deps],
        arguments: AstRewritePreviewRequest,
    ) -> AstRewritePreviewToolResult:
        del context
        preview = await self._provider.preview_rewrite(arguments)
        content = AstRewritePreviewToolContent(preview=preview).model_dump(mode='json')
        return AstRewritePreviewToolResult(content=content)


class AstEditApplyTool[Deps](BaseTool[Deps, AstRewriteApplyRequest, AstRewriteApplyToolResult]):
    id = 'ast_edit_apply'
    description = AST_EDIT_APPLY_DESCRIPTION
    args_type = AstRewriteApplyRequest
    result_type = AstRewriteApplyToolResult
    approval = ToolApproval(required=True, reason='Apply a staged structural rewrite to workspace files')
    timeout_seconds = 30.0
    defer_loading = True

    def __init__(self, *, provider: WorkspaceAstProvider) -> None:
        self._provider = provider

    async def execute(
        self,
        context: ToolExecutionContext[Deps],
        arguments: AstRewriteApplyRequest,
    ) -> AstRewriteApplyToolResult:
        del context
        result = await self._provider.apply_rewrite(arguments)
        content = AstRewriteApplyToolContent(result=result).model_dump(mode='json')
        return AstRewriteApplyToolResult(content=content)
