import asyncio
from collections.abc import Sequence
from typing import Any

from ovid_core.runtime.context import RunContext
from ovid_core.tools.base import BaseTool, BaseToolset, ToolExecutionContext
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
    AstSearchToolContent,
    AstSearchToolResult,
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
from ovid_native.workspace.models import WorkspaceAstProvider
from ovid_native.workspace.observations import WorkspaceLineRange, WorkspaceObservationService


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

    def __init__(
        self,
        *,
        provider: WorkspaceAstProvider,
        presenter: WorkspaceSourcePresenter | None = None,
    ) -> None:
        self._provider = provider
        self._presenter = presenter

    async def execute(
        self,
        context: ToolExecutionContext[Deps],
        arguments: AstSearchRequest,
    ) -> AstSearchToolResult:
        del context
        result = await self._provider.search(arguments)
        content_model = AstSearchToolContent(result=result).model_dump(mode='json')
        if self._presenter is None:
            return AstSearchToolResult(content=content_model)
        groups = await _ast_source_groups(result, self._presenter)
        if self._presenter.presentation.format == 'hashline':
            content = '\n\n'.join(group.render(self._presenter.presentation) for group in groups)
            return AstSearchToolResult(content=content, metadata=content_model)
        return AstSearchToolResult(content=content_model)


class AstSourceToolset[Deps](BaseToolset[Deps]):
    id = 'native_ast_source'

    def __init__(
        self,
        *,
        provider: WorkspaceAstProvider,
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
        return _BoundAstSourceToolset(
            owner=self,
            tool=AstGrepTool(provider=self._provider, presenter=presenter),
        )

    async def get_tools(self, context: RunContext[Deps]) -> Sequence[BaseTool[Deps, Any, Any]]:
        return await (await self.for_step(context)).get_tools(context)


class _BoundAstSourceToolset[Deps](BaseToolset[Deps]):
    id = 'native_ast_source'

    def __init__(self, *, owner: AstSourceToolset[Deps], tool: BaseTool[Deps, Any, Any]) -> None:
        self._owner = owner
        self._tool = tool

    async def for_step(self, context: RunContext[Deps]) -> BaseToolset[Deps]:
        return await self._owner.for_step(context)

    async def get_tools(self, context: RunContext[Deps]) -> Sequence[BaseTool[Deps, Any, Any]]:
        del context
        return (self._tool,)


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
            end_line=match.range.end.line,
            end_byte=match.range.end.byte_offset,
        )
        for match in matches
    )
    evidence = WorkspaceEvidence(
        path=path,
        revision=None,
        lines=tuple(WorkspaceSourceLineClaim(line_number=line, text=text) for line, text in sorted(claimed.items())),
        visible_ranges=_line_ranges(tuple(claimed)),
        spans=spans,
    )
    return await presenter.observe(
        WorkspaceObservationRequest(
            evidence=evidence,
            purpose='ast_grep',
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
