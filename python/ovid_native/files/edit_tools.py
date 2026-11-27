from abc import abstractmethod
from collections.abc import Awaitable

from ovid_core.models import BaseModel
from ovid_core.tools.base import BaseTool, ToolExecutionContext, ToolGrammar, ToolPresentation
from ovid_core.tools.models import ToolApproval, ToolEffect
from pydantic import TypeAdapter

from ovid_native import _native
from ovid_native.files.hashline import HASHLINE_EXAMPLES, HASHLINE_GRAMMAR
from ovid_native.files.models import (
    ApplyPatchEditRequest,
    HashlineEditRequest,
    PatchEditRequest,
    ReplaceEditRequest,
    WorkspaceEditResult,
    WorkspaceFilesToolResult,
    WorkspacePostEditSource,
)
from ovid_native.files.tool_metadata import WorkspaceEditToolMetadata
from ovid_native.workspace.evidence import WorkspaceSourcePresentation, observation_metadata
from ovid_native.workspace.models import WorkspaceFilesProvider


_REPLACE_DESCRIPTION = (
    'Replace the smallest unique old string in an existing workspace file. Add surrounding context when the old string '
    'is ambiguous. Every changed source line must have been rendered and remain unchanged.'
)
_PATCH_DESCRIPTION = (
    'Apply structured patch hunks to one workspace path. Hunk rows use @@ headers and space, -, or + prefixes. '
    'Existing source lines must have been rendered. The complete request is preflighted before commit. Example: '
    '{"path": "src/app.py", "edits": [{"operation": "update", "diff": "@@\\n def run():\\n-    return 1\\n+    '
    'return 2"}]}'
)
_APPLY_PATCH_DESCRIPTION = (
    'Apply a multi-file patch envelope beginning with *** Begin Patch and ending with *** End Patch. Use Add File, '
    'Update File, optional Move to, and Delete File headers. Existing source lines must have been rendered. Example:\n'
    '*** Begin Patch\n*** Update File: src/app.py\n@@\n def run():\n-    return 1\n+    return 2\n'
    '*** Add File: src/new.py\n+value = 1\n*** Delete File: src/old.py\n*** End Patch'
)
_HASHLINE_DESCRIPTION = (
    'Edit workspace files with a Hashline patch:\n'
    '*** Begin Patch\n[path#TAG]\nstatements\n*** End Patch\n'
    'Write one [path#TAG] header per file. TAG is the 4-hex tag from the latest render of that file. In the path, '
    'write ] as \\] and \\ as \\\\. Anchors A and B are LINE:hh, copied from rendered LINE:hh|text rows. All line '
    'numbers refer to the file before this request.\n'
    'Statements:\n'
    'PUT A.=B: replaces lines A to B inclusive.\n'
    'PUT A*: replaces the syntax block that starts at A.\n'
    'PUT <A: and PUT >A: insert before or after line A. PUT >A*: inserts after the block that starts at A.\n'
    'PUT <^: and PUT >$: insert at the start or end of the file.\n'
    'Body rows follow these statements and start with +. The text after + is the line. A lone + is a blank line.\n'
    'CUT A.=B [@name] and CUT A* [@name] delete lines or a block and keep them in a register.\n'
    'PUT LOC [@name], with no colon and no body, pastes a register at any PUT locator LOC, a range included.\n'
    'Without @name, the anonymous register is used. It lasts for one request. Named registers persist between '
    'requests in the same session. In one request, put each CUT before the paste that uses it.\n'
    'REM deletes the file. MV dest moves the file to a new path. Use a JSON string when dest has spaces. Use one REM '
    'or MV per file, after a complete render of the file.\n'
    'Replaced ranges, replaced blocks, and cuts must not overlap. The complete request is checked before the first '
    'write. Changed, unseen, or missing anchors fail, so reread the file. Use write to create a file. Anchors '
    're-rendered in an edit result replace earlier anchors.\n'
    f'Examples:\n{"".join(HASHLINE_EXAMPLES)}'
)


class _WorkspaceEditTool[Deps, Args: BaseModel](BaseTool[Deps, Args, WorkspaceFilesToolResult]):
    result_type = WorkspaceFilesToolResult
    presentation = ToolPresentation(wire_name='edit')
    approval = ToolApproval(required=True, reason='Modify workspace files')
    effect = ToolEffect(kind='workspace_write', replay='unsafe')
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
        arguments: Args,
    ) -> WorkspaceFilesToolResult:
        del context
        result = await self._apply(arguments)
        return tool_edit_result(result, self._presentation)

    @abstractmethod
    def _apply(self, arguments: Args) -> Awaitable[WorkspaceEditResult]: ...


class ReplaceEditTool[Deps](_WorkspaceEditTool[Deps, ReplaceEditRequest]):
    id = 'native_files_replace'
    description = _REPLACE_DESCRIPTION
    args_type = ReplaceEditRequest

    def _apply(self, arguments: ReplaceEditRequest) -> Awaitable[WorkspaceEditResult]:
        return self._provider.replace(arguments, mutation=self._mutation)


class PatchEditTool[Deps](_WorkspaceEditTool[Deps, PatchEditRequest]):
    id = 'native_files_patch'
    description = _PATCH_DESCRIPTION
    args_type = PatchEditRequest

    def _apply(self, arguments: PatchEditRequest) -> Awaitable[WorkspaceEditResult]:
        return self._provider.patch(arguments, mutation=self._mutation)


class ApplyPatchEditTool[Deps](_WorkspaceEditTool[Deps, ApplyPatchEditRequest]):
    id = 'native_files_apply_patch'
    description = _APPLY_PATCH_DESCRIPTION
    args_type = ApplyPatchEditRequest
    presentation = ToolPresentation(wire_name='edit', input_format='text')

    def _apply(self, arguments: ApplyPatchEditRequest) -> Awaitable[WorkspaceEditResult]:
        return self._provider.apply_patch(arguments, mutation=self._mutation)


class HashlineEditTool[Deps](_WorkspaceEditTool[Deps, HashlineEditRequest]):
    id = 'native_files_hashline'
    description = _HASHLINE_DESCRIPTION
    args_type = HashlineEditRequest
    presentation = ToolPresentation(
        wire_name='edit',
        input_format='text',
        grammar=ToolGrammar(syntax='lark', definition=HASHLINE_GRAMMAR),
    )

    def _apply(self, arguments: HashlineEditRequest) -> Awaitable[WorkspaceEditResult]:
        return self._provider.hashline(arguments, mutation=self._mutation)


_EDIT_TOOL_METADATA_ADAPTER = TypeAdapter(WorkspaceEditToolMetadata)


def tool_edit_result(
    edit: WorkspaceEditResult,
    presentation: WorkspaceSourcePresentation,
) -> WorkspaceFilesToolResult:
    rendered = {source.path for source in edit.post_edit_sources}
    sections = [_render_post(source, presentation) for source in edit.post_edit_sources]
    unrendered = [
        f'{change.operation}: {change.path} -> {change.destination}'
        if change.destination
        else f'{change.operation}: {change.path}'
        for change in edit.changes
        if (change.destination or change.path) not in rendered
    ]
    if unrendered:
        sections.append(', '.join(unrendered))
    if edit.matching_strategy == 'fuzzy' and edit.confidence is not None:
        sections.append(f'[Fuzzy match applied with confidence {edit.confidence:.2f}. Check the edited lines]')
    if not edit.commit_complete:
        sections.append('[Commit incomplete: only the changes shown were written. Reread the files before you retry]')
    metadata = _EDIT_TOOL_METADATA_ADAPTER.validate_python(
        {
            **edit.model_dump(mode='json', exclude={'post_edit_sources'}),
            'source_presentation': presentation.model_dump(mode='json'),
            'observations': [
                observation_metadata(source.observation, [line.line_number for line in source.lines])
                for source in edit.post_edit_sources
            ],
        }
    )
    return WorkspaceFilesToolResult(content='\n\n'.join(sections), metadata=metadata)


def _render_post(source: WorkspacePostEditSource, presentation: WorkspaceSourcePresentation) -> str:
    if presentation.format == 'hashline':
        return source.render()

    rows = [f'[{source.path}]']
    rows.extend(f'{line.line_number}:{line.text}' for line in source.lines)

    return '\n'.join(rows)
