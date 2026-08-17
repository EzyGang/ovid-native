from collections.abc import Callable
from typing import cast

from ovid_native import _native
from ovid_native._native_execution import run_native
from ovid_native.files.hashline import parse_hashline
from ovid_native.files.models import (
    ApplyPatchEditRequest,
    HashlineEditRequest,
    PatchEditRequest,
    ReplaceEditRequest,
    WorkspaceCreateRequest,
    WorkspaceDeleteRequest,
    WorkspaceEditResult,
    WorkspaceMoveRequest,
    WorkspaceReplaceRequest,
    WorkspaceWriteResult,
)
from ovid_native.files.results import WorkspaceEditResultMapper
from ovid_native.workspace.errors import _NATIVE_ERRORS, WorkspaceClosedError, translate_native_workspace_error
from ovid_native.workspace.models import WorkspaceMutation, WorkspaceSessionId
from ovid_native.workspace.observations import NativeWorkspaceChangeEvents


class WorkspaceFilesWorkflows(WorkspaceEditResultMapper):
    def __init__(
        self,
        workspace: _native.NativeWorkspace,
        *,
        session_id: WorkspaceSessionId,
        change_events: NativeWorkspaceChangeEvents,
    ) -> None:
        super().__init__(_session_id=session_id, _change_events=change_events)
        self._workspace = workspace

    async def create_file(self, request: WorkspaceCreateRequest) -> WorkspaceWriteResult:
        native = await self._mutate(
            lambda cancellation: _native.workspace_create_file(
                self._workspace,
                request.path,
                request.content,
                request.create_parents,
                cancellation,
            )
        )
        return self._write_result(native)

    async def replace_file(self, request: WorkspaceReplaceRequest) -> WorkspaceWriteResult:
        native = await self._mutate(
            lambda cancellation: _native.workspace_replace_file(
                self._workspace,
                request.path,
                request.content,
                request.expected_observation,
                cancellation,
            )
        )
        return self._write_result(native)

    async def delete_file(self, request: WorkspaceDeleteRequest) -> WorkspaceWriteResult:
        native = await self._mutate(
            lambda cancellation: _native.workspace_delete_file(self._workspace, request.path, cancellation)
        )
        return self._write_result(native)

    async def move_file(self, request: WorkspaceMoveRequest) -> WorkspaceWriteResult:
        native = await self._mutate(
            lambda cancellation: _native.workspace_move_file(
                self._workspace,
                request.path,
                request.destination,
                cancellation,
            )
        )
        return self._write_result(native)

    async def replace(
        self,
        request: ReplaceEditRequest,
        *,
        mutation: WorkspaceMutation | None = None,
    ) -> WorkspaceEditResult:
        captured = self._capture(mutation, mode='replace')
        native = await self._mutate(
            lambda cancellation: _native.workspace_replace_text(
                self._workspace,
                captured,
                request.path,
                (request.old_string, request.new_string, request.replace_all),
                cancellation,
            )
        )
        return self._edit_result(native)

    async def patch(
        self,
        request: PatchEditRequest,
        *,
        mutation: WorkspaceMutation | None = None,
    ) -> WorkspaceEditResult:
        captured = self._capture(mutation, mode='patch')
        edits: list[tuple[str, str | None, str | None]] = [
            (entry.operation, entry.diff, entry.destination) for entry in request.edits
        ]
        native = await self._mutate(
            lambda cancellation: _native.workspace_patch(
                self._workspace,
                captured,
                request.path,
                edits,
                cancellation,
            )
        )
        return self._edit_result(native)

    async def apply_patch(
        self,
        request: ApplyPatchEditRequest,
        *,
        mutation: WorkspaceMutation | None = None,
    ) -> WorkspaceEditResult:
        captured = self._capture(mutation, mode='apply_patch')
        native = await self._mutate(
            lambda cancellation: _native.workspace_apply_patch(
                self._workspace,
                captured,
                request.input,
                cancellation,
            )
        )
        return self._edit_result(native)

    async def hashline(
        self,
        request: HashlineEditRequest,
        *,
        mutation: WorkspaceMutation | None = None,
    ) -> WorkspaceEditResult:
        captured = self._capture(mutation, mode='hashline')
        sections = parse_hashline(request.input)
        native = await self._mutate(
            lambda cancellation: _native.workspace_hashline(
                self._workspace,
                captured,
                sections,
                cancellation,
            )
        )
        return self._edit_result(native)

    def _capture(self, mutation: WorkspaceMutation | None, *, mode: str) -> _native.NativeWorkspaceMutation:
        if mutation is not None:
            return cast(_native.NativeWorkspaceMutation, mutation)
        return _native.workspace_capture_mutation(self._workspace, mode)

    async def _mutate[Result](
        self,
        operation: Callable[[_native.NativeWorkspaceCancellation], Result],
    ) -> Result:
        cancellation = _native.NativeWorkspaceCancellation()
        self._ensure_open()
        try:
            return await run_native(lambda: operation(cancellation), cancellation=cancellation)
        except _native.NativeWorkspacePartialCommitError as error:
            _, _, _, native_changes = error.args
            self._publish_changes(tuple(self._change(change) for change in native_changes))
            raise translate_native_workspace_error(error) from error
        except _NATIVE_ERRORS as error:
            raise translate_native_workspace_error(error) from error

    def _ensure_open(self) -> None:
        if _native.workspace_is_closed(self._workspace):
            raise WorkspaceClosedError('Workspace session is closed')
