from ovid_core.models import BaseModel
from ovid_core.tools.base import BaseTool, ToolExecutionContext
from ovid_core.tools.models import ToolApproval

from ovid_native.files.edit_modes import EditModeState
from ovid_native.files.edit_tools import tool_edit_result
from ovid_native.files.models import (
    WorkspaceCreateRequest,
    WorkspaceDirectoryTreeReadRequest,
    WorkspaceFileReadRequest,
    WorkspaceFilesToolResult,
    WorkspaceReadRequest,
    WorkspaceReplaceRequest,
    WorkspaceWriteRequest,
)
from ovid_native.files.read_results import (
    RenderedReadTarget,
    render_directory_read,
    render_file_read,
    render_read_error,
)
from ovid_native.files.read_targets import (
    MAX_READ_LINES,
    PlannedReadTarget,
    parse_read_target,
    plan_read_target,
    split_read_targets,
)
from ovid_native.files.tool_metadata import WorkspaceFilesToolMetadata
from ovid_native.workspace.errors import WorkspaceError, WorkspacePathError
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
    'Read bounded UTF-8 workspace files or hierarchical directory trees. Append :N, :N-M, :N+K, :N-, or '
    'comma-separated ranges. Separate multiple targets with semicolons. Reads return at most 300 lines per open range '
    'and 3000 lines per call, with exact continuation selectors. Nested directories show at most 12 children. Read a '
    'subdirectory path to expand an abbreviated node.'
)
_WRITE_DESCRIPTION = (
    'Create a workspace text file, or replace a complete existing file guarded by the four-hex observation from read.'
)


class _FilePresentationTool[Deps, Args: BaseModel](BaseTool[Deps, Args, WorkspaceFilesToolResult]):
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

    def _current_presentation(self) -> WorkspaceSourcePresentation:
        if self._state is not None:
            selection = self._state.current
            return capture_source_presentation(selection.mode, selection.generation)
        return self._presentation or capture_source_presentation('apply_patch', 1)


class ReadTool[Deps](_FilePresentationTool[Deps, WorkspaceReadRequest]):
    id = 'read'
    description = _READ_DESCRIPTION
    args_type = WorkspaceReadRequest

    async def execute(
        self,
        context: ToolExecutionContext[Deps],
        arguments: WorkspaceReadRequest,
    ) -> WorkspaceFilesToolResult:
        del context
        presentation = self._current_presentation()
        try:
            raw_targets = (arguments.path,)
            if ';' in arguments.path:
                try:
                    literal = plan_read_target(parse_read_target(arguments.path, arguments.ranges), MAX_READ_LINES)
                    rendered = await self._read_target(literal, arguments.directory_depth, presentation)
                except WorkspacePathError:
                    raw_targets = split_read_targets(arguments.path)
                else:
                    return _read_tool_result((rendered,))
        except WorkspaceError as error:
            return _read_tool_result((render_read_error(arguments.path, error),))

        line_budget = max(1, MAX_READ_LINES // len(raw_targets))
        rendered_targets: list[RenderedReadTarget] = []
        for argument in raw_targets:
            try:
                target = plan_read_target(parse_read_target(argument, arguments.ranges), line_budget)
                rendered_targets.append(await self._read_target(target, arguments.directory_depth, presentation))
            except WorkspaceError as error:
                rendered_targets.append(render_read_error(argument, error))

        return _read_tool_result(tuple(rendered_targets))

    async def _read_target(
        self,
        target: PlannedReadTarget,
        directory_depth: int,
        presentation: WorkspaceSourcePresentation,
    ) -> RenderedReadTarget:
        try:
            result = await self._provider.read_file(WorkspaceFileReadRequest(path=target.path, ranges=target.ranges))
        except WorkspacePathError as file_error:
            try:
                directory = await self._provider.read_directory_tree(
                    WorkspaceDirectoryTreeReadRequest(path=target.path, depth=directory_depth)
                )
            except WorkspacePathError:
                raise file_error from None

            return render_directory_read(directory, target)

        return render_file_read(result, presentation, target)


class WriteTool[Deps](_FilePresentationTool[Deps, WorkspaceWriteRequest]):
    id = 'write'
    description = _WRITE_DESCRIPTION
    args_type = WorkspaceWriteRequest
    approval = ToolApproval(required=True, reason='Create or replace a workspace file')

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


def _replace_request(arguments: WorkspaceWriteRequest) -> WorkspaceReplaceRequest:
    return WorkspaceReplaceRequest(
        path=arguments.path,
        content=arguments.content,
        expected_observation=arguments.expected_observation or '',
    )


def _read_tool_result(targets: tuple[RenderedReadTarget, ...]) -> WorkspaceFilesToolResult:
    if len(targets) == 1:
        target = targets[0]
        return WorkspaceFilesToolResult(content=target.content, metadata=target.metadata)

    successful = sum(target.metadata['status'] == 'ok' for target in targets)
    metadata: WorkspaceFilesToolMetadata = {
        'kind': 'multi',
        'target_count': len(targets),
        'successful_targets': successful,
        'failed_targets': len(targets) - successful,
        'truncated': any(target.metadata.get('truncated') is True for target in targets),
        'targets': [target.metadata for target in targets],
    }
    return WorkspaceFilesToolResult(
        content='\n\n'.join(target.content for target in targets),
        metadata=metadata,
    )
