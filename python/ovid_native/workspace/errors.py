from ovid_native import _native


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


_NATIVE_ERRORS: tuple[type[Exception], ...] = (
    _native.NativeWorkspaceReadError,
    _native.NativeWorkspaceEncodingError,
    _native.NativeWorkspaceBinaryFileError,
    _native.NativeWorkspaceLimitError,
    _native.NativeWorkspaceObservationNotFoundError,
    _native.NativeWorkspaceObservationCollisionError,
    _native.NativeWorkspaceUnseenLineError,
    _native.NativeWorkspaceObservedLineChangedError,
    _native.NativeWorkspaceStaleError,
    _native.NativeWorkspaceEditModeError,
    _native.NativeWorkspacePatchError,
    _native.NativeWorkspacePartialCommitError,
    _native.NativeWorkspaceWriteError,
    _native.NativeWorkspacePathError,
    _native.NativeWorkspaceClosedError,
)


def translate_native_workspace_error(error: Exception) -> WorkspaceError:
    mapping: tuple[tuple[type[Exception], type[WorkspaceError]], ...] = (
        (_native.NativeWorkspaceEncodingError, WorkspaceEncodingError),
        (_native.NativeWorkspaceBinaryFileError, WorkspaceBinaryFileError),
        (_native.NativeWorkspaceReadError, WorkspaceReadError),
        (_native.NativeWorkspaceLimitError, WorkspaceLimitError),
        (_native.NativeWorkspaceObservationNotFoundError, WorkspaceObservationNotFoundError),
        (_native.NativeWorkspaceObservationCollisionError, WorkspaceObservationCollisionError),
        (_native.NativeWorkspaceUnseenLineError, WorkspaceUnseenLineError),
        (_native.NativeWorkspaceObservedLineChangedError, WorkspaceObservedLineChangedError),
        (_native.NativeWorkspaceStaleError, WorkspaceStaleError),
        (_native.NativeWorkspaceEditModeError, WorkspaceEditModeError),
        (_native.NativeWorkspacePatchError, WorkspacePatchError),
        (_native.NativeWorkspaceWriteError, WorkspaceWriteError),
        (_native.NativeWorkspacePathError, WorkspacePathError),
        (_native.NativeWorkspaceClosedError, WorkspaceClosedError),
    )
    if isinstance(error, _native.NativeWorkspacePartialCommitError):
        _, landed, pending = error.args
        return WorkspacePartialCommitError(landed=tuple(landed), pending=tuple(pending))
    for native_type, public_type in mapping:
        if isinstance(error, native_type):
            return public_type(str(error))
    return WorkspaceError(str(error))
