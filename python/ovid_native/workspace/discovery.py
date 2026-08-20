from ovid_native import _native
from ovid_native._native_execution import run_native
from ovid_native.workspace.errors import (
    _NATIVE_ERRORS,
    WorkspaceClosedError,
    WorkspaceConfigurationError,
    translate_native_workspace_error,
)
from ovid_native.workspace.models import WorkspaceDiscoveryRequest, WorkspaceDiscoveryResult


class NativeWorkspaceDiscovery:
    def __init__(self, workspace: _native.NativeWorkspace) -> None:
        self._workspace = workspace

    async def discover(self, request: WorkspaceDiscoveryRequest) -> WorkspaceDiscoveryResult:
        if _native.workspace_is_closed(self._workspace):
            raise WorkspaceClosedError('Workspace session is closed')

        cancellation = _native.NativeWorkspaceCancellation()
        try:
            paths, completion = await run_native(
                lambda: _native.workspace_discover_files(
                    self._workspace,
                    request.filename,
                    request.max_depth,
                    request.limit,
                    request.timeout_seconds,
                    cancellation,
                ),
                cancellation=cancellation,
            )
        except ValueError as error:
            raise WorkspaceConfigurationError(str(error)) from error
        except _NATIVE_ERRORS as error:
            raise translate_native_workspace_error(error) from error

        return WorkspaceDiscoveryResult(paths=tuple(paths), completion=completion)
