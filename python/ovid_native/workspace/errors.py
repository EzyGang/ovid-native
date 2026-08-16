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
