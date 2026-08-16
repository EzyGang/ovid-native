from collections.abc import Sequence
from typing import Any

from ovid_core.runtime.context import RunContext
from ovid_core.tools.base import BaseTool, BaseToolset, ToolExecutionContext, ToolPresentation
from ovid_core.tools.models import ToolApproval

from ovid_native import _native
from ovid_native.files.edit_modes import EditMode, EditModeState
from ovid_native.files.engine import WorkspaceFilesEngine
from ovid_native.files.models import (
    ApplyPatchEditRequest,
    PatchEditRequest,
    ReplaceEditRequest,
    WorkspaceCreateRequest,
    WorkspaceEditResult,
    WorkspaceFilesToolResult,
    WorkspaceReadRequest,
    WorkspaceReplaceRequest,
    WorkspaceWriteRequest,
)


FILES_TOOL_INSTRUCTIONS = (
    'Use read for workspace text and directory entries, edit for existing-file changes, and write for explicit file '
    'creation or guarded whole-file replacement. Existing-file mutations are authorized only for exact source lines '
    'previously rendered by read. The edit schema can change between model steps; follow the schema and description '
    'supplied for the current call. Search results do not authorize edits in this iteration.'
)
_READ_DESCRIPTION = (
    'Read bounded UTF-8 workspace text with authorizing line evidence, or list one workspace directory. URLs, '
    'archives, documents, images, databases, SSH paths, and resource schemes are unsupported.'
)
_WRITE_DESCRIPTION = (
    'Create a workspace text file, or replace a complete existing file guarded by the four-hex observation from read.'
)
_REPLACE_DESCRIPTION = (
    'Replace the smallest unique old string in an existing workspace file. Add surrounding context when the old string '
    'is ambiguous. Every changed source line must have been rendered by read and remain unchanged.'
)
_PATCH_DESCRIPTION = (
    'Apply structured patch hunks to one workspace path. Hunk rows use @@ headers and space, -, or + prefixes. '
    'Existing source lines must have been rendered by read. The complete request is preflighted before commit.'
)
_APPLY_PATCH_DESCRIPTION = (
    'Apply a multi-file patch envelope beginning with *** Begin Patch and ending with *** End Patch. Use Add File, '
    'Update File, optional Move to, and Delete File headers. Existing source lines must have been rendered by read.'
)


class ReadTool[Deps](BaseTool[Deps, WorkspaceReadRequest, WorkspaceFilesToolResult]):
    id = 'read'
    description = _READ_DESCRIPTION
    args_type = WorkspaceReadRequest
    result_type = WorkspaceFilesToolResult
    timeout_seconds = 30.0

    def __init__(self, *, provider: WorkspaceFilesEngine) -> None:
        self._provider = provider

    async def execute(
        self,
        context: ToolExecutionContext[Deps],
        arguments: WorkspaceReadRequest,
    ) -> WorkspaceFilesToolResult:
        del context
        result = await self._provider.read(arguments)
        metadata = {'kind': result.kind, 'path': result.path}
        if result.kind == 'file':
            metadata['editable'] = result.editable
            metadata['complete_presentation'] = result.complete_presentation
            metadata['observation'] = None if result.observation is None else result.observation.tag
        else:
            metadata['truncated'] = result.truncated
        return WorkspaceFilesToolResult(content=result.render(), metadata=metadata)


class WriteTool[Deps](BaseTool[Deps, WorkspaceWriteRequest, WorkspaceFilesToolResult]):
    id = 'write'
    description = _WRITE_DESCRIPTION
    args_type = WorkspaceWriteRequest
    result_type = WorkspaceFilesToolResult
    approval = ToolApproval(required=True, reason='Create or replace a workspace file')
    timeout_seconds = 30.0

    def __init__(self, *, provider: WorkspaceFilesEngine) -> None:
        self._provider = provider

    async def execute(
        self,
        context: ToolExecutionContext[Deps],
        arguments: WorkspaceWriteRequest,
    ) -> WorkspaceFilesToolResult:
        del context
        if arguments.operation == 'create':
            result = await self._provider.create_file(
                WorkspaceCreateRequest(
                    path=arguments.path,
                    content=arguments.content,
                    create_parents=arguments.create_parents,
                )
            )
        else:
            result = await self._provider.replace_file(_replace_request(arguments))
        return _tool_edit_result(result)


class _ReplaceEditTool[Deps](BaseTool[Deps, ReplaceEditRequest, WorkspaceFilesToolResult]):
    id = 'native_files_replace'
    description = _REPLACE_DESCRIPTION
    args_type = ReplaceEditRequest
    result_type = WorkspaceFilesToolResult
    presentation = ToolPresentation(wire_name='edit')
    approval = ToolApproval(required=True, reason='Modify workspace files')
    timeout_seconds = 30.0

    def __init__(self, *, provider: WorkspaceFilesEngine, mutation: _native.NativeWorkspaceMutation) -> None:
        self._provider = provider
        self._mutation = mutation

    async def execute(
        self,
        context: ToolExecutionContext[Deps],
        arguments: ReplaceEditRequest,
    ) -> WorkspaceFilesToolResult:
        del context
        return _tool_edit_result(await self._provider.replace(arguments, mutation=self._mutation))


class _PatchEditTool[Deps](BaseTool[Deps, PatchEditRequest, WorkspaceFilesToolResult]):
    id = 'native_files_patch'
    description = _PATCH_DESCRIPTION
    args_type = PatchEditRequest
    result_type = WorkspaceFilesToolResult
    presentation = ToolPresentation(wire_name='edit')
    approval = ToolApproval(required=True, reason='Modify workspace files')
    timeout_seconds = 30.0

    def __init__(self, *, provider: WorkspaceFilesEngine, mutation: _native.NativeWorkspaceMutation) -> None:
        self._provider = provider
        self._mutation = mutation

    async def execute(
        self,
        context: ToolExecutionContext[Deps],
        arguments: PatchEditRequest,
    ) -> WorkspaceFilesToolResult:
        del context
        return _tool_edit_result(await self._provider.patch(arguments, mutation=self._mutation))


class _ApplyPatchEditTool[Deps](BaseTool[Deps, ApplyPatchEditRequest, WorkspaceFilesToolResult]):
    id = 'native_files_apply_patch'
    description = _APPLY_PATCH_DESCRIPTION
    args_type = ApplyPatchEditRequest
    result_type = WorkspaceFilesToolResult
    presentation = ToolPresentation(wire_name='apply_patch', input_format='text')
    approval = ToolApproval(required=True, reason='Modify workspace files')
    timeout_seconds = 30.0

    def __init__(self, *, provider: WorkspaceFilesEngine, mutation: _native.NativeWorkspaceMutation) -> None:
        self._provider = provider
        self._mutation = mutation

    async def execute(
        self,
        context: ToolExecutionContext[Deps],
        arguments: ApplyPatchEditRequest,
    ) -> WorkspaceFilesToolResult:
        del context
        return _tool_edit_result(await self._provider.apply_patch(arguments, mutation=self._mutation))


class EditModeToolset[Deps](BaseToolset[Deps]):
    id = 'native_files_edit_mode'

    def __init__(self, *, provider: WorkspaceFilesEngine, state: EditModeState) -> None:
        self._provider = provider
        self._state = state

    async def for_step(self, context: RunContext[Deps]) -> BaseToolset[Deps]:
        del context
        mutation = self._state.capture()
        mode = EditMode(mutation.mode)
        tools: dict[EditMode, BaseTool[Deps, Any, Any]] = {
            EditMode.REPLACE: _ReplaceEditTool(provider=self._provider, mutation=mutation),
            EditMode.PATCH: _PatchEditTool(provider=self._provider, mutation=mutation),
            EditMode.APPLY_PATCH: _ApplyPatchEditTool(provider=self._provider, mutation=mutation),
        }
        return _BoundEditModeToolset(owner=self, tool=tools[mode])

    async def get_tools(self, context: RunContext[Deps]) -> Sequence[BaseTool[Deps, Any, Any]]:
        return await (await self.for_step(context)).get_tools(context)


class _BoundEditModeToolset[Deps](BaseToolset[Deps]):
    id = 'native_files_edit_mode'

    def __init__(self, *, owner: EditModeToolset[Deps], tool: BaseTool[Deps, Any, Any]) -> None:
        self._owner = owner
        self._tool = tool

    async def for_step(self, context: RunContext[Deps]) -> BaseToolset[Deps]:
        return await self._owner.for_step(context)

    async def get_tools(self, context: RunContext[Deps]) -> Sequence[BaseTool[Deps, Any, Any]]:
        del context
        return (self._tool,)


def _replace_request(arguments: WorkspaceWriteRequest) -> WorkspaceReplaceRequest:
    return WorkspaceReplaceRequest(
        path=arguments.path,
        content=arguments.content,
        expected_observation=arguments.expected_observation or '',
    )


def _tool_edit_result(edit: WorkspaceEditResult) -> WorkspaceFilesToolResult:
    content = '\n\n'.join(source.render() for source in edit.post_edit_sources)
    if not content:
        content = ', '.join(f'{change.operation}: {change.path}' for change in edit.changes)
    metadata = edit.model_dump(mode='json', exclude={'post_edit_sources'})
    return WorkspaceFilesToolResult(content=content, metadata=metadata)
