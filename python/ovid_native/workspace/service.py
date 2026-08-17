import asyncio
import secrets
from collections.abc import Callable, Sequence
from pathlib import Path

from ovid_core.services import AgentServiceBinding

from ovid_native import _native
from ovid_native.ast.engine import AstEngine
from ovid_native.fff.engine import FffEngine
from ovid_native.files.edit_modes import EditMode, EditModeId, EditModeProvider, EditModeState
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
    WorkspaceViewProvider,
)
from ovid_native.workspace.observations import (
    NativeWorkspaceChangeEvents,
    NativeWorkspaceObservationService,
    WorkspaceChangeEvents,
    WorkspaceObservationService,
    WorkspaceObservationStore,
)
from ovid_native.workspace.operations import WorkspaceOperation, workspace_ref
from ovid_native.workspace.policy import WorkspacePolicy, WorkspacePolicyState
from ovid_native.workspace.stores import NativeObservationStore


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
        files_provider: WorkspaceFilesProvider | None = None,
        search_provider: WorkspaceSearchProvider | None = None,
        ast_provider: WorkspaceAstProvider | None = None,
        fff_provider: WorkspaceFffProvider | None = None,
        view_provider: WorkspaceViewProvider | None = None,
        observation_store: WorkspaceObservationStore | None = None,
        edit_mode: EditMode | EditModeId | str = EditMode.APPLY_PATCH,
        edit_mode_providers: Sequence[EditModeProvider] = (),
        policy: WorkspacePolicy | None = None,
        cleanup: Callable[[], None] | None = None,
        enabled_operations: frozenset[WorkspaceOperation] | None = None,
    ) -> None:
        operations = (
            enabled_operations
            if enabled_operations is not None
            else _workspace_operations(has_view=view_provider is not None)
        )
        validated_edit_mode_providers = _validate_edit_mode_providers(edit_mode_providers, operations)

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
        self._edit_mode_providers = self._register_edit_modes(validated_edit_mode_providers)
        self._edit_mode.set(edit_mode)
        self._change_events = NativeWorkspaceChangeEvents(session_id=session_id)
        self._files = files_provider
        if self._files is None and WorkspaceOperation.FILES in operations:
            self._files = WorkspaceFilesEngine(
                native,
                session_id=session_id,
                change_events=self._change_events,
            )
        self._observations: WorkspaceObservationService | None = None
        if WorkspaceOperation.OBSERVATIONS in operations:
            if observation_store is not None:
                if self._files is None:
                    raise WorkspaceConfigurationError('Workspace observation store requires a files provider')
                self._observations = observation_store.bind(session_id=session_id, files=self._files)
            elif files_provider is None:
                self._observations = NativeWorkspaceObservationService(native, session_id=session_id)
            else:
                self._observations = NativeObservationStore().bind(session_id=session_id, files=files_provider)
        self._search = search_provider
        if self._search is None and WorkspaceOperation.SEARCH in operations:
            self._search = SearchEngine._from_workspace(native)
        self._ast = ast_provider
        if self._ast is None and WorkspaceOperation.AST in operations:
            self._ast = AstEngine._from_workspace(native, session_id=session_id.root)
        self._fff = fff_provider
        if self._fff is None and WorkspaceOperation.FFF in operations:
            self._fff = FffEngine._from_workspace(native)
        self._view = view_provider
        self._cleanup = cleanup
        self._operations = operations
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
    def edit_mode_providers(self) -> tuple[EditModeProvider, ...]:
        self._require(WorkspaceOperation.FILES)
        return self._edit_mode_providers

    @property
    def policy(self) -> WorkspacePolicyState:
        self._require(WorkspaceOperation.FILES)
        return self._policy

    @property
    def files(self) -> WorkspaceFilesProvider:
        self._require(WorkspaceOperation.FILES)
        assert self._files is not None
        return self._files

    @property
    def observations(self) -> WorkspaceObservationService:
        self._require(WorkspaceOperation.OBSERVATIONS)
        assert self._observations is not None
        return self._observations

    @property
    def change_events(self) -> WorkspaceChangeEvents:
        self._require(WorkspaceOperation.CHANGE_EVENTS)
        return self._change_events

    @property
    def search(self) -> WorkspaceSearchProvider:
        self._require(WorkspaceOperation.SEARCH)
        assert self._search is not None
        return self._search

    @property
    def ast(self) -> WorkspaceAstProvider:
        self._require(WorkspaceOperation.AST)
        assert self._ast is not None
        return self._ast

    @property
    def fff(self) -> WorkspaceFffProvider:
        self._require(WorkspaceOperation.FFF)
        assert self._fff is not None
        return self._fff

    @property
    def view(self) -> WorkspaceViewProvider:
        self._require(WorkspaceOperation.VIEW)
        assert self._view is not None
        return self._view

    async def close(self) -> None:
        async with self._close_lock:
            if self._closed:
                return
            self._closed = True
            try:
                if self._fff is not None:
                    await self._fff.close()
            finally:
                _native.workspace_close(self._native)
                if self._cleanup is not None:
                    self._cleanup()

    def _register_edit_modes(self, providers: Sequence[EditModeProvider]) -> tuple[EditModeProvider, ...]:
        for provider in providers:
            self._edit_mode.register(provider.id)
        return tuple(providers)

    def _require(self, operation: WorkspaceOperation) -> None:
        if self._closed or _native.workspace_is_closed(self._native):
            raise WorkspaceClosedError('Workspace session is closed')
        if operation not in self._operations:
            raise WorkspaceOperationUnavailableError(f'Workspace operation is unavailable: {operation.value}')


def _workspace_operations(*, has_view: bool) -> frozenset[WorkspaceOperation]:
    operations = {
        WorkspaceOperation.FILES,
        WorkspaceOperation.OBSERVATIONS,
        WorkspaceOperation.CHANGE_EVENTS,
        WorkspaceOperation.SEARCH,
        WorkspaceOperation.AST,
        WorkspaceOperation.FFF,
    }
    if has_view:
        operations.add(WorkspaceOperation.VIEW)
    return frozenset(operations)


def _validate_edit_mode_providers(
    providers: Sequence[EditModeProvider],
    operations: frozenset[WorkspaceOperation],
) -> tuple[EditModeProvider, ...]:
    provider_by_id: dict[str, EditModeProvider] = {}
    for provider in providers:
        if provider.id in provider_by_id:
            raise WorkspaceConfigurationError(f'Duplicate workspace edit mode provider: {provider.id}')
        try:
            EditModeId(provider.id)
        except ValueError as error:
            raise WorkspaceConfigurationError(str(error)) from error
        required = provider.required_operations
        if not isinstance(required, frozenset) or any(not isinstance(item, WorkspaceOperation) for item in required):
            raise WorkspaceConfigurationError(
                f'Workspace edit mode provider has invalid required operations: {provider.id}'
            )
        missing = required - operations
        if missing:
            names = ', '.join(sorted(operation.value for operation in missing))
            raise WorkspaceConfigurationError(
                f'Workspace edit mode provider requires unavailable operations: {provider.id}: {names}'
            )
        provider_by_id[provider.id] = provider
    return tuple(provider_by_id.values())
