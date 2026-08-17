from collections.abc import Callable
from typing import Literal, cast

from ovid_native import _native
from ovid_native._native_execution import run_native
from ovid_native.files.models import (
    WorkspaceDirectoryEntry,
    WorkspaceDirectoryReadRequest,
    WorkspaceFileReadRequest,
    WorkspaceReadDirectoryResult,
    WorkspaceReadFileResult,
    WorkspaceReadRequest,
    WorkspaceReadResult,
)
from ovid_native.files.results import rendered_line
from ovid_native.files.workflows import WorkspaceFilesWorkflows
from ovid_native.workspace.errors import (
    _NATIVE_ERRORS,
    WorkspacePathError,
    WorkspaceReadError,
    translate_native_workspace_error,
)
from ovid_native.workspace.models import WorkspaceSessionId
from ovid_native.workspace.observations import NativeWorkspaceChangeEvents


class WorkspaceFilesEngine(WorkspaceFilesWorkflows):
    def __init__(
        self,
        workspace: _native.NativeWorkspace,
        *,
        session_id: WorkspaceSessionId,
        change_events: NativeWorkspaceChangeEvents,
    ) -> None:
        super().__init__(workspace, session_id=session_id, change_events=change_events)
        self._session_id = session_id

    async def read(self, request: WorkspaceReadRequest) -> WorkspaceReadResult:
        try:
            return await self.read_file(WorkspaceFileReadRequest(path=request.path, ranges=request.ranges))
        except WorkspacePathError as file_error:
            if request.ranges:
                raise WorkspaceReadError('Directory reads do not accept line ranges') from file_error
            try:
                return await self.list_directory(
                    WorkspaceDirectoryReadRequest(path=request.path, depth=request.directory_depth)
                )
            except WorkspacePathError:
                raise file_error from None

    async def read_file(self, request: WorkspaceFileReadRequest) -> WorkspaceReadFileResult:
        native_ranges = [(line_range.start, line_range.end) for line_range in request.ranges]
        native = await self._call(lambda: _native.workspace_read_file(self._workspace, request.path, native_ranges))
        path, receipt, lines, total_lines, complete, editable, total_bytes, observation_limit, serialization = native
        return WorkspaceReadFileResult(
            path=path,
            observation=None if receipt is None else self._receipt(receipt),
            lines=tuple(rendered_line(line) for line in lines),
            total_lines=total_lines,
            complete_presentation=complete,
            editable=editable,
            total_bytes=total_bytes,
            observation_limit=observation_limit,
            serialization=(
                None
                if serialization is None
                else {'bom': serialization[0], 'line_ending': serialization[1], 'terminal_newline': serialization[2]}
            ),
        )

    async def list_directory(self, request: WorkspaceDirectoryReadRequest) -> WorkspaceReadDirectoryResult:
        native = await self._call(
            lambda: _native.workspace_list_directory(self._workspace, request.path, request.depth)
        )
        path, entries, truncated = native
        return WorkspaceReadDirectoryResult(
            path=path,
            entries=tuple(
                WorkspaceDirectoryEntry(
                    path=entry[0],
                    kind=cast(Literal['file', 'directory', 'symlink'], entry[1]),
                    size=entry[2],
                )
                for entry in entries
            ),
            truncated=truncated,
        )

    async def _call[Result](self, operation: Callable[[], Result]) -> Result:
        self._ensure_open()
        try:
            return await run_native(operation)
        except _NATIVE_ERRORS as error:
            raise translate_native_workspace_error(error) from error
