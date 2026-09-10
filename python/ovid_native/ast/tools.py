import asyncio
from collections.abc import Sequence
from typing import Any

from ovid_core.tools.base import BaseTool, ToolExecutionContext
from ovid_core.tools.models import ToolApproval

from ovid_native.ast.models import (
    AstMatch,
    AstRewriteApplyRequest,
    AstRewriteApplyToolContent,
    AstRewriteApplyToolResult,
    AstRewritePreviewRequest,
    AstRewritePreviewToolContent,
    AstRewritePreviewToolResult,
    AstSearchRequest,
    AstSearchResult,
    AstSearchToolResult,
)
from ovid_native.workspace._toolsets import WorkspaceResultTool, WorkspaceSourceTool, WorkspaceSourceToolset
from ovid_native.workspace.evidence import (
    EditableSourceGroup,
    WorkspaceSourcePresenter,
    WorkspaceSourceSpanClaim,
    normalize_terminal_span_end,
)
from ovid_native.workspace.models import WorkspaceAstProvider


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


class AstGrepTool[Deps](WorkspaceSourceTool[Deps, WorkspaceAstProvider, AstSearchRequest, AstSearchToolResult]):
    id = 'ast_grep'
    description = AST_GREP_DESCRIPTION
    args_type = AstSearchRequest
    result_type = AstSearchToolResult
    timeout_seconds = 30.0
    defer_loading = True

    async def execute(
        self,
        context: ToolExecutionContext[Deps],
        arguments: AstSearchRequest,
    ) -> AstSearchToolResult:
        del context
        result = await self._provider.search(arguments)
        return await self._present_result(result, _ast_source_groups)


class AstSourceToolset[Deps](WorkspaceSourceToolset[Deps, WorkspaceAstProvider]):
    id = 'native_ast_source'

    def _source_tools(self, presenter: WorkspaceSourcePresenter) -> Sequence[BaseTool[Deps, Any, Any]]:
        return (AstGrepTool(provider=self._provider, presenter=presenter),)


class AstEditPreviewTool[Deps](
    WorkspaceResultTool[Deps, WorkspaceAstProvider, AstRewritePreviewRequest, AstRewritePreviewToolResult]
):
    id = 'ast_edit_preview'
    description = AST_EDIT_PREVIEW_DESCRIPTION
    args_type = AstRewritePreviewRequest
    result_type = AstRewritePreviewToolResult
    timeout_seconds = 30.0
    defer_loading = True

    async def _content(self, arguments: AstRewritePreviewRequest) -> AstRewritePreviewToolContent:
        return AstRewritePreviewToolContent(preview=await self._provider.preview_rewrite(arguments))


class AstEditApplyTool[Deps](
    WorkspaceResultTool[Deps, WorkspaceAstProvider, AstRewriteApplyRequest, AstRewriteApplyToolResult]
):
    id = 'ast_edit_apply'
    description = AST_EDIT_APPLY_DESCRIPTION
    args_type = AstRewriteApplyRequest
    result_type = AstRewriteApplyToolResult
    approval = ToolApproval(required=True, reason='Apply a staged structural rewrite to workspace files')
    timeout_seconds = 30.0
    defer_loading = True

    async def _content(self, arguments: AstRewriteApplyRequest) -> AstRewriteApplyToolContent:
        return AstRewriteApplyToolContent(result=await self._provider.apply_rewrite(arguments))


async def _ast_source_groups(
    result: AstSearchResult,
    presenter: WorkspaceSourcePresenter,
) -> tuple[EditableSourceGroup, ...]:
    grouped: dict[str, list[AstMatch]] = {}
    for match in result.matches:
        grouped.setdefault(match.path, []).append(match)
    return tuple(
        await asyncio.gather(*(_observe_ast_path(path, matches, presenter) for path, matches in grouped.items()))
    )


async def _observe_ast_path(
    path: str,
    matches: list[AstMatch],
    presenter: WorkspaceSourcePresenter,
) -> EditableSourceGroup:
    claimed = {line.line_number: line.text for match in matches for line in match.source_lines}
    spans = tuple(
        WorkspaceSourceSpanClaim(
            start_line=match.range.start.line,
            start_byte=match.range.start.byte_offset,
            end_line=end_line,
            end_byte=end_byte,
        )
        for match in matches
        for end_line, end_byte in (
            normalize_terminal_span_end(
                end_line=match.range.end.line,
                end_column=match.range.end.column,
                end_byte=match.range.end.byte_offset,
                claimed_lines=claimed,
            ),
        )
    )
    return await presenter.observe_lines(path=path, lines=claimed, spans=spans, purpose='ast_grep')
