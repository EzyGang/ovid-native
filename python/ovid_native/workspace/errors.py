from ovid_native import _native
from ovid_native._native_execution import NativeErrorTranslator


class WorkspaceError(Exception):
    pass


class WorkspaceConfigurationError(WorkspaceError):
    pass


class WorkspacePathError(WorkspaceError):
    pass


class WorkspaceOperationUnavailableError(WorkspaceError):
    pass


class WorkspaceClosedError(WorkspaceError):
    pass


class WorkspaceReadError(WorkspaceError):
    pass


class WorkspaceEncodingError(WorkspaceReadError):
    pass


class WorkspaceBinaryFileError(WorkspaceReadError):
    pass


class WorkspaceLimitError(WorkspaceError):
    pass


class WorkspaceObservationError(WorkspaceError):
    pass


class WorkspaceObservationNotFoundError(WorkspaceObservationError):
    pass


class WorkspaceObservationCollisionError(WorkspaceObservationError):
    pass


class WorkspaceUnseenLineError(WorkspaceObservationError):
    pass


class WorkspaceObservedLineChangedError(WorkspaceObservationError):
    pass


class WorkspaceStaleError(WorkspaceError):
    pass


class WorkspaceEditModeError(WorkspaceError):
    pass


class WorkspacePatchError(WorkspaceError):
    pass


class WorkspaceWriteError(WorkspaceError):
    pass


class WorkspacePartialCommitError(WorkspaceError):
    def __init__(self, *, landed: tuple[str, ...], pending: tuple[str, ...]) -> None:
        super().__init__('Workspace patch committed only part of its operations')
        self.landed = landed
        self.pending = pending


_ERROR_TRANSLATOR = NativeErrorTranslator(
    {
        _native.NativeWorkspaceReadError: WorkspaceReadError,
        _native.NativeWorkspaceEncodingError: WorkspaceEncodingError,
        _native.NativeWorkspaceBinaryFileError: WorkspaceBinaryFileError,
        _native.NativeWorkspaceLimitError: WorkspaceLimitError,
        _native.NativeWorkspaceObservationNotFoundError: WorkspaceObservationNotFoundError,
        _native.NativeWorkspaceObservationCollisionError: WorkspaceObservationCollisionError,
        _native.NativeWorkspaceUnseenLineError: WorkspaceUnseenLineError,
        _native.NativeWorkspaceObservedLineChangedError: WorkspaceObservedLineChangedError,
        _native.NativeWorkspaceStaleError: WorkspaceStaleError,
        _native.NativeWorkspaceEditModeError: WorkspaceEditModeError,
        _native.NativeWorkspacePatchError: WorkspacePatchError,
        _native.NativeWorkspacePartialCommitError: WorkspaceError,
        _native.NativeWorkspaceWriteError: WorkspaceWriteError,
        _native.NativeWorkspacePathError: WorkspacePathError,
        _native.NativeWorkspaceClosedError: WorkspaceClosedError,
    },
    fallback=WorkspaceError,
)


def translate_native_workspace_error(error: Exception) -> WorkspaceError:
    if isinstance(error, _native.NativeWorkspacePartialCommitError):
        _, landed, pending, _ = error.args
        return WorkspacePartialCommitError(landed=tuple(landed), pending=tuple(pending))
    return _ERROR_TRANSLATOR(error)
