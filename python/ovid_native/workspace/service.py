import asyncio
import secrets
from pathlib import Path

from ovid_core.services import AgentServiceBinding

from ovid_native import _native
from ovid_native.ast.engine import AstEngine
from ovid_native.fff.engine import FffEngine
from ovid_native.runtime import ensure_native_compatibility
from ovid_native.search.engine import SearchEngine
from ovid_native.workspace.errors import (
    WorkspaceClosedError,
    WorkspaceConfigurationError,
    WorkspaceOperationUnavailableError,
)
from ovid_native.workspace.models import (
    WorkspaceAstProvider,
    WorkspaceFffProvider,
    WorkspaceSearchProvider,
    WorkspaceSession,
    WorkspaceSessionId,
)
from ovid_native.workspace.operations import WorkspaceOperation, workspace_ref


def workspace_binding(
    session: WorkspaceSession,
    *,
    name: str = 'default',
) -> AgentServiceBinding[WorkspaceSession]:
    return AgentServiceBinding(
        ref=workspace_ref(name),
        value=session,
        provider=type(session).__qualname__,
        features=frozenset(operation.value for operation in session.operations),
        identity=session.id.root,
    )


class NativeWorkspaceSession:
    def __init__(
        self,
        *,
        root: Path,
        search_provider: WorkspaceSearchProvider | None = None,
        ast_provider: WorkspaceAstProvider | None = None,
        fff_provider: WorkspaceFffProvider | None = None,
    ) -> None:
        ensure_native_compatibility()
        try:
            native = _native.workspace_create(str(root))
        except ValueError as error:
            raise WorkspaceConfigurationError(str(error)) from error
        session_id = WorkspaceSessionId(secrets.token_urlsafe(24))
        self._native = native
        self._id = session_id
        self._search = search_provider if search_provider is not None else SearchEngine._from_workspace(native)
        self._ast = (
            ast_provider
            if ast_provider is not None
            else AstEngine._from_workspace(
                native,
                session_id=session_id.root,
            )
        )
        self._fff = fff_provider if fff_provider is not None else FffEngine._from_workspace(native)
        self._operations = frozenset(
            (
                WorkspaceOperation.SEARCH,
                WorkspaceOperation.AST,
                WorkspaceOperation.FFF,
            )
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
        self._require(WorkspaceOperation.SEARCH)
        return self._search

    @property
    def ast(self) -> WorkspaceAstProvider:
        self._require(WorkspaceOperation.AST)
        return self._ast

    @property
    def fff(self) -> WorkspaceFffProvider:
        self._require(WorkspaceOperation.FFF)
        return self._fff

    async def close(self) -> None:
        async with self._close_lock:
            if self._closed:
                return

            self._closed = True
            try:
                await self._fff.close()
            finally:
                _native.workspace_close(self._native)

    def _require(self, operation: WorkspaceOperation) -> None:
        if self._closed or _native.workspace_is_closed(self._native):
            raise WorkspaceClosedError('Workspace session is closed')
        if operation not in self._operations:
            raise WorkspaceOperationUnavailableError(f'Workspace operation is unavailable: {operation.value}')
