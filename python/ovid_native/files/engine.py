from collections.abc import Callable
from typing import Literal, cast

from ovid_native import _native
from ovid_native._native_execution import run_native
from ovid_native.files.models import (
    ApplyPatchEditRequest,
    PatchEditRequest,
    ReplaceEditRequest,
    WorkspaceCreateRequest,
    WorkspaceDeleteRequest,
    WorkspaceDirectoryEntry,
    WorkspaceDirectoryReadRequest,
    WorkspaceEditResult,
    WorkspaceFileChange,
    WorkspaceFileReadRequest,
    WorkspaceMoveRequest,
    WorkspacePostEditSource,
    WorkspaceReadDirectoryResult,
    WorkspaceReadFileResult,
    WorkspaceReadRequest,
    WorkspaceReadResult,
    WorkspaceReplaceRequest,
    WorkspaceWriteResult,
)
from ovid_native.workspace.errors import (
    _NATIVE_ERRORS,
    WorkspaceClosedError,
    WorkspacePathError,
    WorkspaceReadError,
    translate_native_workspace_error,
)
from ovid_native.workspace.models import WorkspaceSessionId
from ovid_native.workspace.observations import (
    NativeWorkspaceChangeEvents,
    NativeWorkspaceObservationService,
    WorkspaceRenderedLine,
)


class WorkspaceFilesEngine:
    def __init__(
        self,
        workspace: _native.NativeWorkspace,
        *,
        session_id: WorkspaceSessionId,
        observations: NativeWorkspaceObservationService,
        change_events: NativeWorkspaceChangeEvents,
    ) -> None:
        self._workspace = workspace
        self._session_id = session_id
        self._observations = observations
        self._change_events = change_events

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
        path, receipt, lines, total_lines, complete, editable, total_bytes, observation_limit = native
        return WorkspaceReadFileResult(
            path=path,
            observation=None if receipt is None else self._observations.receipt(receipt),
            lines=tuple(_rendered_line(line) for line in lines),
            total_lines=total_lines,
            complete_presentation=complete,
            editable=editable,
            total_bytes=total_bytes,
            observation_limit=observation_limit,
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

    async def create_file(self, request: WorkspaceCreateRequest) -> WorkspaceWriteResult:
        native = await self._call(
            lambda: _native.workspace_create_file(
                self._workspace,
                request.path,
                request.content,
                request.create_parents,
            )
        )
        return self._write_result(native)

    async def replace_file(self, request: WorkspaceReplaceRequest) -> WorkspaceWriteResult:
        native = await self._call(
            lambda: _native.workspace_replace_file(
                self._workspace,
                request.path,
                request.content,
                request.expected_observation,
            )
        )
        return self._write_result(native)

    async def delete_file(self, request: WorkspaceDeleteRequest) -> WorkspaceWriteResult:
        native = await self._call(lambda: _native.workspace_delete_file(self._workspace, request.path))
        return self._write_result(native)

    async def move_file(self, request: WorkspaceMoveRequest) -> WorkspaceWriteResult:
        native = await self._call(
            lambda: _native.workspace_move_file(self._workspace, request.path, request.destination)
        )
        return self._write_result(native)

    async def replace(
        self,
        request: ReplaceEditRequest,
        *,
        mutation: _native.NativeWorkspaceMutation | None = None,
    ) -> WorkspaceEditResult:
        captured = mutation if mutation is not None else _native.workspace_capture_mutation(self._workspace)
        native = await self._call(
            lambda: _native.workspace_replace_text(
                self._workspace,
                captured,
                request.path,
                request.old_string,
                request.new_string,
                request.replace_all,
            )
        )
        return self._edit_result(native)

    async def patch(
        self,
        request: PatchEditRequest,
        *,
        mutation: _native.NativeWorkspaceMutation | None = None,
    ) -> WorkspaceEditResult:
        captured = mutation if mutation is not None else _native.workspace_capture_mutation(self._workspace)
        edits: list[tuple[str, str | None, str | None]] = [
            (entry.operation, entry.diff, entry.destination) for entry in request.edits
        ]
        native = await self._call(lambda: _native.workspace_patch(self._workspace, captured, request.path, edits))
        return self._edit_result(native)

    async def apply_patch(
        self,
        request: ApplyPatchEditRequest,
        *,
        mutation: _native.NativeWorkspaceMutation | None = None,
    ) -> WorkspaceEditResult:
        captured = mutation if mutation is not None else _native.workspace_capture_mutation(self._workspace)
        native = await self._call(lambda: _native.workspace_apply_patch(self._workspace, captured, request.input))
        return self._edit_result(native)

    def _write_result(self, native: _native.NativeWorkspaceEditResult) -> WorkspaceWriteResult:
        result = self._edit_result(native)
        return WorkspaceWriteResult.model_validate(result.model_dump())

    def _edit_result(self, native: _native.NativeWorkspaceEditResult) -> WorkspaceEditResult:
        mode, mode_generation, policy_generation, changes, posts, preflight, commit, strategy, confidence = native
        mapped_changes = tuple(self._change(change) for change in changes)
        result = WorkspaceEditResult(
            mode=mode,
            mode_generation=mode_generation,
            policy_generation=policy_generation,
            changes=mapped_changes,
            post_edit_sources=tuple(self._post_source(post) for post in posts),
            preflight_complete=preflight,
            commit_complete=commit,
            matching_strategy=cast(Literal['exact', 'fuzzy'] | None, strategy),
            confidence=confidence,
        )
        for change in mapped_changes:
            self._change_events.publish(
                path=change.path,
                operation=change.operation,
                destination=change.destination,
                generation=change.file_generation,
                revision=change.revision,
            )
        return result

    def _change(self, native: _native.NativeWorkspaceFileChange) -> WorkspaceFileChange:
        path, operation, destination, before, after, receipt, generation, revision = native
        return WorkspaceFileChange(
            path=path,
            operation=cast(Literal['create', 'update', 'delete', 'move'], operation),
            destination=destination,
            before_sha256=before,
            after_sha256=after,
            observation=None if receipt is None else self._observations.receipt(receipt),
            file_generation=generation,
            revision=revision,
        )

    def _post_source(self, native: _native.NativeWorkspacePostEditSource) -> WorkspacePostEditSource:
        path, receipt, lines, complete = native
        return WorkspacePostEditSource(
            path=path,
            observation=self._observations.receipt(receipt),
            lines=tuple(_rendered_line(line) for line in lines),
            complete_presentation=complete,
        )

    async def _call[Result](self, operation: Callable[[], Result]) -> Result:
        self._ensure_open()
        try:
            return await run_native(operation)
        except _NATIVE_ERRORS as error:
            raise translate_native_workspace_error(error) from error

    def _ensure_open(self) -> None:
        if _native.workspace_is_closed(self._workspace):
            raise WorkspaceClosedError('Workspace session is closed')


def _rendered_line(native: _native.NativeWorkspaceRenderedLine) -> WorkspaceRenderedLine:
    return WorkspaceRenderedLine(line_number=native[0], short_hash=native[1], text=native[2])
