import asyncio
from pathlib import Path

from ovid_native import _native
from ovid_native._native_execution import run_native
from ovid_native.ast.engine import AstEngine
from ovid_native.ast.models import AstLimits
from ovid_native.fff.engine import FffEngine
from ovid_native.fff.models import FffConfig, FffLimits
from ovid_native.runtime import ensure_native_compatibility
from ovid_native.search.engine import SearchEngine
from ovid_native.search.models import SearchLimits
from ovid_native.workspace.errors import (
    WorkspaceClosedError,
    WorkspaceConfigurationError,
    WorkspaceOperationUnavailableError,
    WorkspacePathError,
)
from ovid_native.workspace.models import WorkspaceOperation, WorkspaceSessionId
from ovid_native.workspace.operations import WorkspaceAstProvider, WorkspaceFffProvider, WorkspaceSearchProvider


class NativeWorkspaceSession:
    def __init__(
        self,
        *,
        root: Path,
        search_limits: SearchLimits | None = None,
        ast_limits: AstLimits | None = None,
        fff_config: FffConfig = FffConfig(),
        fff_limits: FffLimits = FffLimits(),
    ) -> None:
        self._initialize(
            root=root,
            search_limits=search_limits,
            ast_limits=ast_limits,
            fff_config=fff_config,
            fff_limits=fff_limits,
            search_provider=None,
            ast_provider=None,
            fff_provider=None,
        )

    @classmethod
    def _configured(
        cls,
        *,
        root: Path,
        search_provider: WorkspaceSearchProvider | None,
        ast_provider: WorkspaceAstProvider | None,
        fff_provider: WorkspaceFffProvider | None,
    ) -> NativeWorkspaceSession:
        session = cls.__new__(cls)
        session._initialize(
            root=root,
            search_limits=None,
            ast_limits=None,
            fff_config=FffConfig(),
            fff_limits=FffLimits(),
            search_provider=search_provider,
            ast_provider=ast_provider,
            fff_provider=fff_provider,
        )
        return session

    def _initialize(
        self,
        *,
        root: Path,
        search_limits: SearchLimits | None,
        ast_limits: AstLimits | None,
        fff_config: FffConfig,
        fff_limits: FffLimits,
        search_provider: WorkspaceSearchProvider | None,
        ast_provider: WorkspaceAstProvider | None,
        fff_provider: WorkspaceFffProvider | None,
    ) -> None:
        ensure_native_compatibility()
        self._id = WorkspaceSessionId.new()
        self._native = _create_native_workspace(root, self._id)
        self._operations = frozenset(
            {
                WorkspaceOperation.SEARCH,
                WorkspaceOperation.AST,
                WorkspaceOperation.FFF,
            }
        )
        _validate_native_provider_workspaces(
            self._native,
            search_provider=search_provider,
            ast_provider=ast_provider,
            fff_provider=fff_provider,
        )
        self._search = (
            search_provider
            if search_provider is not None
            else SearchEngine._from_workspace(self._native, limits=search_limits)
        )
        self._ast = (
            ast_provider if ast_provider is not None else AstEngine._from_workspace(self._native, limits=ast_limits)
        )
        self._fff = (
            fff_provider
            if fff_provider is not None
            else FffEngine._from_workspace(self._native, config=fff_config, limits=fff_limits)
        )
        self._closed = False
        self._close_lock = asyncio.Lock()

    @property
    def id(self) -> WorkspaceSessionId:
        return self._id

    @property
    def operations(self) -> frozenset[WorkspaceOperation]:
        return self._operations

    @property
    def search(self) -> WorkspaceSearchProvider:
        self._ensure_available(WorkspaceOperation.SEARCH)
        return self._search

    @property
    def ast(self) -> WorkspaceAstProvider:
        self._ensure_available(WorkspaceOperation.AST)
        return self._ast

    @property
    def fff(self) -> WorkspaceFffProvider:
        self._ensure_available(WorkspaceOperation.FFF)
        return self._fff

    async def close(self) -> None:
        async with self._close_lock:
            if self._closed:
                return

            self._closed = True
            await run_native(lambda: _native.workspace_close(self._native))
            await self._fff.close()

    def _ensure_available(self, operation: WorkspaceOperation) -> None:
        if self._closed:
            raise WorkspaceClosedError('Workspace session is closed')
        if operation not in self._operations:
            raise WorkspaceOperationUnavailableError(f'Workspace operation is unavailable: {operation.value}')


def _validate_native_provider_workspaces(
    workspace: _native.NativeWorkspace,
    *,
    search_provider: WorkspaceSearchProvider | None,
    ast_provider: WorkspaceAstProvider | None,
    fff_provider: WorkspaceFffProvider | None,
) -> None:
    providers = (search_provider, ast_provider, fff_provider)

    for provider in providers:
        if isinstance(provider, (SearchEngine, AstEngine, FffEngine)) and provider._workspace is not workspace:
            raise WorkspaceConfigurationError('Native workspace provider must use the configured workspace session')


def _create_native_workspace(root: Path, session_id: WorkspaceSessionId) -> _native.NativeWorkspace:
    try:
        return _native.workspace_create(str(root), session_id.root)
    except (_native.NativeWorkspaceConfigurationError, _native.NativeWorkspacePathError) as error:
        raise _workspace_error(error) from error


def _workspace_error(error: Exception) -> WorkspaceConfigurationError | WorkspacePathError:
    if isinstance(error, _native.NativeWorkspacePathError):
        return WorkspacePathError(str(error))

    return WorkspaceConfigurationError(str(error))
