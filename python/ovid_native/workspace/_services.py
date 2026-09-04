from dataclasses import dataclass

from ovid_native import _native
from ovid_native.ast.engine import AstEngine
from ovid_native.ast.models import AstLimits
from ovid_native.command.engine import CommandEngine
from ovid_native.fff.engine import FffEngine
from ovid_native.files.edit_modes import EditModeState
from ovid_native.files.engine import WorkspaceFilesEngine
from ovid_native.search.engine import SearchEngine
from ovid_native.workspace.errors import WorkspaceConfigurationError
from ovid_native.workspace.models import (
    WorkspaceAstProvider,
    WorkspaceCommandProvider,
    WorkspaceFffProvider,
    WorkspaceFilesProvider,
    WorkspaceSearchProvider,
    WorkspaceSessionId,
    WorkspaceViewProvider,
)
from ovid_native.workspace.observations import (
    NativeWorkspaceChangeEvents,
    NativeWorkspaceObservationService,
    WorkspaceObservationService,
    WorkspaceObservationStore,
)
from ovid_native.workspace.operations import WorkspaceOperation
from ovid_native.workspace.stores import NativeObservationStore
from ovid_native.workspace.views import NativeViewAstProvider


@dataclass(frozen=True, slots=True)
class WorkspaceProviderOverrides:
    files: WorkspaceFilesProvider | None
    command: WorkspaceCommandProvider | None
    search: WorkspaceSearchProvider | None
    ast: WorkspaceAstProvider | None
    fff: WorkspaceFffProvider | None
    view: WorkspaceViewProvider | None


class NativeWorkspaceServices:
    def __init__(
        self,
        *,
        workspace: _native.NativeWorkspace,
        session_id: WorkspaceSessionId,
        change_events: NativeWorkspaceChangeEvents,
        operations: frozenset[WorkspaceOperation],
        overrides: WorkspaceProviderOverrides,
        ast_limits: AstLimits | None,
        edit_mode: EditModeState,
        observation_store: WorkspaceObservationStore | None,
    ) -> None:
        self.command = overrides.command
        if self.command is None and WorkspaceOperation.COMMAND in operations:
            self.command = CommandEngine(workspace)

        self.files = overrides.files
        if self.files is None and WorkspaceOperation.FILES in operations:
            self.files = WorkspaceFilesEngine(workspace, session_id=session_id, change_events=change_events)

        self.search = overrides.search
        if self.search is None and WorkspaceOperation.SEARCH in operations:
            self.search = SearchEngine._from_workspace(workspace)

        self.ast = overrides.ast
        if self.ast is None and WorkspaceOperation.AST in operations:
            self.ast = AstEngine._from_workspace(workspace, session_id=session_id.root, limits=ast_limits)
        if isinstance(self.ast, NativeViewAstProvider):
            self.ast.bind_edit_mode(edit_mode)

        self.fff = overrides.fff
        if self.fff is None and WorkspaceOperation.FFF in operations:
            self.fff = FffEngine._from_workspace(workspace)
        self.view = overrides.view
        self.observations = self._bind_observations(
            workspace=workspace,
            session_id=session_id,
            operations=operations,
            store=observation_store,
            custom_files=overrides.files is not None,
        )

    async def close(self) -> None:
        try:
            if isinstance(self.command, CommandEngine):
                await self.command.close()
        finally:
            if self.fff is not None:
                await self.fff.close()

    def _bind_observations(
        self,
        *,
        workspace: _native.NativeWorkspace,
        session_id: WorkspaceSessionId,
        operations: frozenset[WorkspaceOperation],
        store: WorkspaceObservationStore | None,
        custom_files: bool,
    ) -> WorkspaceObservationService | None:
        if WorkspaceOperation.OBSERVATIONS not in operations:
            return None
        if store is not None:
            if self.files is None:
                raise WorkspaceConfigurationError('Workspace observation store requires a files provider')
            return store.bind(session_id=session_id, files=self.files)
        if custom_files:
            assert self.files is not None
            return NativeObservationStore().bind(session_id=session_id, files=self.files)
        return NativeWorkspaceObservationService(workspace, session_id=session_id)
