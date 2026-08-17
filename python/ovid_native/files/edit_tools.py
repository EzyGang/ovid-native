from ovid_core.tools.base import BaseTool, ToolExecutionContext, ToolGrammar, ToolPresentation
from ovid_core.tools.models import ToolApproval

from ovid_native import _native
from ovid_native.files.hashline import HASHLINE_GRAMMAR
from ovid_native.files.models import (
    ApplyPatchEditRequest,
    HashlineEditRequest,
    PatchEditRequest,
    ReplaceEditRequest,
    WorkspaceEditResult,
    WorkspaceFilesToolResult,
    WorkspacePostEditSource,
)
from ovid_native.workspace.evidence import WorkspaceSourcePresentation
from ovid_native.workspace.models import WorkspaceFilesProvider


_REPLACE_DESCRIPTION = (
    'Replace the smallest unique old string in an existing workspace file. Add surrounding context when the old string '
    'is ambiguous. Every changed source line must have been rendered and remain unchanged.'
)
_PATCH_DESCRIPTION = (
    'Apply structured patch hunks to one workspace path. Hunk rows use @@ headers and space, -, or + prefixes. '
    'Existing source lines must have been rendered. The complete request is preflighted before commit.'
)
_APPLY_PATCH_DESCRIPTION = (
    'Apply a multi-file patch envelope beginning with *** Begin Patch and ending with *** End Patch. Use Add File, '
    'Update File, optional Move to, and Delete File headers. Existing source lines must have been rendered.'
)
_HASHLINE_DESCRIPTION = (
    'Edit exact rendered workspace lines with a Hashline patch. Copy the four-hex file tag and line-qualified two-hex '
    'anchors exactly. Changed, shifted, unseen, missing, or ambiguous anchors require rereading or rerunning the '
    'source-producing tool. The complete multi-file request is preflighted before the first commit.'
)


class ReplaceEditTool[Deps](BaseTool[Deps, ReplaceEditRequest, WorkspaceFilesToolResult]):
    id = 'native_files_replace'
    description = _REPLACE_DESCRIPTION
    args_type = ReplaceEditRequest
    result_type = WorkspaceFilesToolResult
    presentation = ToolPresentation(wire_name='edit')
    approval = ToolApproval(required=True, reason='Modify workspace files')
    timeout_seconds = 30.0

    def __init__(
        self,
        *,
        provider: WorkspaceFilesProvider,
        mutation: _native.NativeWorkspaceMutation,
        presentation: WorkspaceSourcePresentation,
    ) -> None:
        self._provider = provider
        self._mutation = mutation
        self._presentation = presentation

    async def execute(
        self,
        context: ToolExecutionContext[Deps],
        arguments: ReplaceEditRequest,
    ) -> WorkspaceFilesToolResult:
        del context
        result = await self._provider.replace(arguments, mutation=self._mutation)
        return tool_edit_result(result, self._presentation)


class PatchEditTool[Deps](BaseTool[Deps, PatchEditRequest, WorkspaceFilesToolResult]):
    id = 'native_files_patch'
    description = _PATCH_DESCRIPTION
    args_type = PatchEditRequest
    result_type = WorkspaceFilesToolResult
    presentation = ToolPresentation(wire_name='edit')
    approval = ToolApproval(required=True, reason='Modify workspace files')
    timeout_seconds = 30.0

    def __init__(
        self,
        *,
        provider: WorkspaceFilesProvider,
        mutation: _native.NativeWorkspaceMutation,
        presentation: WorkspaceSourcePresentation,
    ) -> None:
        self._provider = provider
        self._mutation = mutation
        self._presentation = presentation

    async def execute(
        self,
        context: ToolExecutionContext[Deps],
        arguments: PatchEditRequest,
    ) -> WorkspaceFilesToolResult:
        del context
        result = await self._provider.patch(arguments, mutation=self._mutation)
        return tool_edit_result(result, self._presentation)


class ApplyPatchEditTool[Deps](BaseTool[Deps, ApplyPatchEditRequest, WorkspaceFilesToolResult]):
    id = 'native_files_apply_patch'
    description = _APPLY_PATCH_DESCRIPTION
    args_type = ApplyPatchEditRequest
    result_type = WorkspaceFilesToolResult
    presentation = ToolPresentation(wire_name='edit', input_format='text')
    approval = ToolApproval(required=True, reason='Modify workspace files')
    timeout_seconds = 30.0

    def __init__(
        self,
        *,
        provider: WorkspaceFilesProvider,
        mutation: _native.NativeWorkspaceMutation,
        presentation: WorkspaceSourcePresentation,
    ) -> None:
        self._provider = provider
        self._mutation = mutation
        self._presentation = presentation

    async def execute(
        self,
        context: ToolExecutionContext[Deps],
        arguments: ApplyPatchEditRequest,
    ) -> WorkspaceFilesToolResult:
        del context
        result = await self._provider.apply_patch(arguments, mutation=self._mutation)
        return tool_edit_result(result, self._presentation)


class HashlineEditTool[Deps](BaseTool[Deps, HashlineEditRequest, WorkspaceFilesToolResult]):
    id = 'native_files_hashline'
    description = _HASHLINE_DESCRIPTION
    args_type = HashlineEditRequest
    result_type = WorkspaceFilesToolResult
    presentation = ToolPresentation(
        wire_name='edit',
        input_format='text',
        grammar=ToolGrammar(syntax='lark', definition=HASHLINE_GRAMMAR),
    )
    approval = ToolApproval(required=True, reason='Modify workspace files')
    timeout_seconds = 30.0

    def __init__(
        self,
        *,
        provider: WorkspaceFilesProvider,
        mutation: _native.NativeWorkspaceMutation,
        presentation: WorkspaceSourcePresentation,
    ) -> None:
        self._provider = provider
        self._mutation = mutation
        self._presentation = presentation

    async def execute(
        self,
        context: ToolExecutionContext[Deps],
        arguments: HashlineEditRequest,
    ) -> WorkspaceFilesToolResult:
        del context
        result = await self._provider.hashline(arguments, mutation=self._mutation)
        return tool_edit_result(result, self._presentation)


def tool_edit_result(
    edit: WorkspaceEditResult,
    presentation: WorkspaceSourcePresentation,
) -> WorkspaceFilesToolResult:
    content = '\n\n'.join(_render_post(source, presentation) for source in edit.post_edit_sources)
    if not content:
        content = ', '.join(f'{change.operation}: {change.path}' for change in edit.changes)
    metadata = edit.model_dump(mode='json', exclude={'post_edit_sources'})
    metadata['source_presentation'] = presentation.model_dump(mode='json')
    return WorkspaceFilesToolResult(content=content, metadata=metadata)


def _render_post(source: WorkspacePostEditSource, presentation: WorkspaceSourcePresentation) -> str:
    if presentation.format == 'hashline':
        return source.render()
    rows = [f'[{source.path}]']
    rows.extend(f'{line.line_number}:{line.text}' for line in source.lines)
    return '\n'.join(rows)
