from ovid_core.tools.base import BaseTool, ToolExecutionContext
from ovid_core.tools.models import ToolApproval

from ovid_native.files.edit_modes import EditModeState
from ovid_native.files.edit_tools import tool_edit_result
from ovid_native.files.models import (
    WorkspaceCreateRequest,
    WorkspaceFilesToolResult,
    WorkspaceReadFileResult,
    WorkspaceReadRequest,
    WorkspaceReplaceRequest,
    WorkspaceWriteRequest,
)
from ovid_native.workspace.evidence import WorkspaceSourcePresentation, capture_source_presentation
from ovid_native.workspace.models import WorkspaceFilesProvider


FILES_TOOL_INSTRUCTIONS = (
    'Use read for workspace text and directory entries, edit for existing-file changes, and write for explicit file '
    'creation or guarded whole-file replacement. Existing-file mutations reject source lines that were not rendered '
    'or that changed after rendering. The edit schema and source presentation can change between model steps; follow '
    'the current definitions. In Hashline mode, exact source from read, grep, FFF grep, or AST grep is rendered as '
    '`[path#4hex]` followed by `LINE:2hex|text` and may be edited directly. Path-only search results do not authorize '
    'edits.'
)
_READ_DESCRIPTION = (
    'Read bounded UTF-8 workspace text with authorizing line evidence, or list one workspace directory. URLs, '
    'archives, documents, images, databases, SSH paths, and resource schemes are unsupported.'
)
_WRITE_DESCRIPTION = (
    'Create a workspace text file, or replace a complete existing file guarded by the four-hex observation from read.'
)


class ReadTool[Deps](BaseTool[Deps, WorkspaceReadRequest, WorkspaceFilesToolResult]):
    id = 'read'
    description = _READ_DESCRIPTION
    args_type = WorkspaceReadRequest
    result_type = WorkspaceFilesToolResult
    timeout_seconds = 30.0

    def __init__(
        self,
        *,
        provider: WorkspaceFilesProvider,
        presentation: WorkspaceSourcePresentation | None = None,
        state: EditModeState | None = None,
    ) -> None:
        self._provider = provider
        self._presentation = presentation
        self._state = state

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
        content = _render_read(result, self._current_presentation()) if result.kind == 'file' else result.render()
        return WorkspaceFilesToolResult(content=content, metadata=metadata)

    def _current_presentation(self) -> WorkspaceSourcePresentation:
        return _current_presentation(self._presentation, self._state)


class WriteTool[Deps](BaseTool[Deps, WorkspaceWriteRequest, WorkspaceFilesToolResult]):
    id = 'write'
    description = _WRITE_DESCRIPTION
    args_type = WorkspaceWriteRequest
    result_type = WorkspaceFilesToolResult
    approval = ToolApproval(required=True, reason='Create or replace a workspace file')
    timeout_seconds = 30.0

    def __init__(
        self,
        *,
        provider: WorkspaceFilesProvider,
        presentation: WorkspaceSourcePresentation | None = None,
        state: EditModeState | None = None,
    ) -> None:
        self._provider = provider
        self._presentation = presentation
        self._state = state

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
        return tool_edit_result(result, self._current_presentation())

    def _current_presentation(self) -> WorkspaceSourcePresentation:
        return _current_presentation(self._presentation, self._state)


def _current_presentation(
    presentation: WorkspaceSourcePresentation | None,
    state: EditModeState | None,
) -> WorkspaceSourcePresentation:
    if state is not None:
        selection = state.current
        return capture_source_presentation(selection.mode, selection.generation)
    return presentation or capture_source_presentation('apply_patch', 1)


def _replace_request(arguments: WorkspaceWriteRequest) -> WorkspaceReplaceRequest:
    return WorkspaceReplaceRequest(
        path=arguments.path,
        content=arguments.content,
        expected_observation=arguments.expected_observation or '',
    )


def _render_read(result: WorkspaceReadFileResult, presentation: WorkspaceSourcePresentation) -> str:
    if presentation.format == 'hashline' and result.observation is not None and result.editable:
        return result.render()
    rows = [f'[{result.path}]']
    rows.extend(f'{line.line_number}:{line.text}' for line in result.lines)
    if not result.complete_presentation:
        rows.append(f'[truncated: {len(result.lines)} of {result.total_lines} lines]')
    return '\n'.join(rows)
