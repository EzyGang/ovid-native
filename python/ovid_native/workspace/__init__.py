from importlib import import_module
from typing import TYPE_CHECKING, Any


if TYPE_CHECKING:
    from ovid_native.workspace.builder import WorkspaceSessionBuilder as WorkspaceSessionBuilder
    from ovid_native.workspace.errors import WorkspaceClosedError as WorkspaceClosedError
    from ovid_native.workspace.errors import WorkspaceConfigurationError as WorkspaceConfigurationError
    from ovid_native.workspace.errors import WorkspaceError as WorkspaceError
    from ovid_native.workspace.errors import WorkspaceOperationUnavailableError as WorkspaceOperationUnavailableError
    from ovid_native.workspace.errors import WorkspacePathError as WorkspacePathError
    from ovid_native.workspace.models import WorkspaceOperation as WorkspaceOperation
    from ovid_native.workspace.models import WorkspaceSessionId as WorkspaceSessionId
    from ovid_native.workspace.models import WorkspaceView as WorkspaceView
    from ovid_native.workspace.models import WorkspaceViewPurpose as WorkspaceViewPurpose
    from ovid_native.workspace.native import NativeWorkspaceSession as NativeWorkspaceSession
    from ovid_native.workspace.operations import WorkspaceAstProvider as WorkspaceAstProvider
    from ovid_native.workspace.operations import WorkspaceFffProvider as WorkspaceFffProvider
    from ovid_native.workspace.operations import WorkspaceSearchProvider as WorkspaceSearchProvider
    from ovid_native.workspace.operations import WorkspaceViewProvider as WorkspaceViewProvider
    from ovid_native.workspace.service import WORKSPACE_SERVICE_KEY as WORKSPACE_SERVICE_KEY
    from ovid_native.workspace.service import WorkspaceSession as WorkspaceSession
    from ovid_native.workspace.service import workspace_binding as workspace_binding
    from ovid_native.workspace.service import workspace_ref as workspace_ref


_EXPORT_MODULES = {
    'NativeWorkspaceSession': 'ovid_native.workspace.native',
    'WorkspaceSessionBuilder': 'ovid_native.workspace.builder',
    'WorkspaceClosedError': 'ovid_native.workspace.errors',
    'WorkspaceConfigurationError': 'ovid_native.workspace.errors',
    'WorkspaceError': 'ovid_native.workspace.errors',
    'WorkspaceOperationUnavailableError': 'ovid_native.workspace.errors',
    'WorkspacePathError': 'ovid_native.workspace.errors',
    'WorkspaceOperation': 'ovid_native.workspace.models',
    'WorkspaceSessionId': 'ovid_native.workspace.models',
    'WorkspaceView': 'ovid_native.workspace.models',
    'WorkspaceViewPurpose': 'ovid_native.workspace.models',
    'WorkspaceAstProvider': 'ovid_native.workspace.operations',
    'WorkspaceFffProvider': 'ovid_native.workspace.operations',
    'WorkspaceSearchProvider': 'ovid_native.workspace.operations',
    'WorkspaceViewProvider': 'ovid_native.workspace.operations',
    'WORKSPACE_SERVICE_KEY': 'ovid_native.workspace.service',
    'WorkspaceSession': 'ovid_native.workspace.service',
    'workspace_binding': 'ovid_native.workspace.service',
    'workspace_ref': 'ovid_native.workspace.service',
}


def __getattr__(name: str) -> Any:  # noqa: ANN401
    module_name = _EXPORT_MODULES.get(name)
    if module_name is None:
        raise AttributeError(f'module {__name__!r} has no attribute {name!r}')

    value = getattr(import_module(module_name), name)
    globals()[name] = value
    return value


def __dir__() -> list[str]:
    return sorted((*globals(), *_EXPORT_MODULES))
