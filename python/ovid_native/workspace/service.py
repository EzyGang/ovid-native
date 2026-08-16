import asyncio
import secrets
from pathlib import Path

from ovid_core.services import AgentServiceBinding

from ovid_native import _native
from ovid_native.ast.engine import AstEngine
from ovid_native.fff.engine import FffEngine
from ovid_native.files.edit_modes import EditMode, EditModeState
from ovid_native.files.engine import WorkspaceFilesEngine
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
    WorkspaceFilesProvider,
    WorkspaceSearchProvider,
    WorkspaceSession,
    WorkspaceSessionId,
)
from ovid_native.workspace.observations import (
    NativeWorkspaceChangeEvents,
    NativeWorkspaceObservationService,
    WorkspaceChangeEvents,
    WorkspaceObservationService,
)
from ovid_native.workspace.operations import WorkspaceOperation, workspace_ref
from ovid_native.workspace.policy import WorkspacePolicy, WorkspacePolicyState


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
        edit_mode: EditMode = EditMode.APPLY_PATCH,
        policy: WorkspacePolicy | None = None,
    ) -> None:
        ensure_native_compatibility()
        try:
            native = _native.workspace_create(str(root))
        except ValueError as error:
            raise WorkspaceConfigurationError(str(error)) from error
        session_id = WorkspaceSessionId(secrets.token_urlsafe(24))
        self._native = native
        self._id = session_id
        self._policy = WorkspacePolicyState(native)
        if policy is not None:
            self._policy.set(policy)
        self._edit_mode = EditModeState(native)
        self._edit_mode.set(edit_mode)
        self._observations = NativeWorkspaceObservationService(native, session_id=session_id)
        self._change_events = NativeWorkspaceChangeEvents(session_id=session_id)
        self._files = WorkspaceFilesEngine(
            native,
            session_id=session_id,
            observations=self._observations,
            change_events=self._change_events,
        )
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
                WorkspaceOperation.FILES,
                WorkspaceOperation.OBSERVATIONS,
                WorkspaceOperation.CHANGE_EVENTS,
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
    def edit_mode(self) -> EditModeState:
        self._require(WorkspaceOperation.FILES)
        return self._edit_mode

    @property
    def policy(self) -> WorkspacePolicyState:
        self._require(WorkspaceOperation.FILES)
        return self._policy

    @property
    def files(self) -> WorkspaceFilesProvider:
        self._require(WorkspaceOperation.FILES)
        return self._files

    @property
    def observations(self) -> WorkspaceObservationService:
        self._require(WorkspaceOperation.OBSERVATIONS)
        return self._observations

    @property
    def change_events(self) -> WorkspaceChangeEvents:
        self._require(WorkspaceOperation.CHANGE_EVENTS)
        return self._change_events

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
