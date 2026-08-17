from collections.abc import Callable
from pathlib import Path
from tempfile import TemporaryDirectory
from typing import Self

from ovid_native.files.edit_modes import EditMode, EditModeId, EditModeProvider
from ovid_native.workspace.errors import WorkspaceConfigurationError
from ovid_native.workspace.models import (
    WorkspaceAstProvider,
    WorkspaceFffProvider,
    WorkspaceFilesProvider,
    WorkspaceSearchProvider,
    WorkspaceViewProvider,
)
from ovid_native.workspace.observations import WorkspaceObservationStore
from ovid_native.workspace.operations import WorkspaceOperation
from ovid_native.workspace.policy import WorkspacePolicy
from ovid_native.workspace.service import NativeWorkspaceSession
from ovid_native.workspace.views import NativeViewAstProvider, NativeViewFffProvider, NativeViewSearchProvider


class WorkspaceSessionBuilder:
    def __init__(
        self,
        *,
        root: Path | None = None,
        edit_mode: EditMode | EditModeId | str = EditMode.HASHLINE,
        policy: WorkspacePolicy | None = None,
    ) -> None:
        self._root = root
        self._edit_mode = edit_mode
        self._policy = policy
        self._files_provider: WorkspaceFilesProvider | None = None
        self._search_provider: WorkspaceSearchProvider | None = None
        self._ast_provider: WorkspaceAstProvider | None = None
        self._fff_provider: WorkspaceFffProvider | None = None
        self._view_provider: WorkspaceViewProvider | None = None
        self._observation_store: WorkspaceObservationStore | None = None
        self._native_selected: set[str] = set()
        self._selected: set[str] = set()
        self._edit_mode_providers: dict[str, EditModeProvider] = {}
        self._built = False

    @classmethod
    def native(
        cls,
        *,
        root: Path,
        edit_mode: EditMode | EditModeId | str = EditMode.APPLY_PATCH,
        policy: WorkspacePolicy | None = None,
    ) -> Self:
        return cls(root=root, edit_mode=edit_mode, policy=policy)

    def with_files_provider(self, provider: WorkspaceFilesProvider) -> Self:
        self._select('files')
        _require_methods(
            provider,
            operation='files',
            methods=(
                'read',
                'read_file',
                'list_directory',
                'create_file',
                'replace_file',
                'delete_file',
                'move_file',
                'replace',
                'patch',
                'apply_patch',
                'hashline',
            ),
        )
        self._files_provider = provider
        return self

    def with_search_provider(self, provider: WorkspaceSearchProvider) -> Self:
        self._select('search')
        _require_methods(provider, operation='search', methods=('glob', 'grep'))
        self._search_provider = provider
        return self

    def with_ast_provider(self, provider: WorkspaceAstProvider) -> Self:
        self._select('ast')
        _require_methods(provider, operation='ast', methods=('search', 'preview_rewrite', 'apply_rewrite'))
        self._ast_provider = provider
        return self

    def with_fff_provider(self, provider: WorkspaceFffProvider) -> Self:
        self._select('fff')
        _require_methods(
            provider,
            operation='fff',
            methods=('start', 'wait_ready', 'find', 'grep', 'multi_grep', 'close'),
        )
        self._fff_provider = provider
        return self

    def with_view_provider(self, provider: WorkspaceViewProvider) -> Self:
        self._select('view')
        _require_methods(provider, operation='view', methods=('acquire_view', 'current_revision'))
        self._view_provider = provider
        return self

    def with_observation_store(self, store: WorkspaceObservationStore) -> Self:
        self._select('observations')
        _require_methods(store, operation='observations', methods=('bind',))
        self._observation_store = store
        return self

    def with_native_search(self) -> Self:
        self._select('search')
        self._native_selected.add('search')
        return self

    def with_native_ast(self) -> Self:
        self._select('ast')
        self._native_selected.add('ast')
        return self

    def with_native_fff(self) -> Self:
        self._select('fff')
        self._native_selected.add('fff')
        return self

    def with_edit_mode(self, mode: EditMode | EditModeId | str) -> Self:
        self._edit_mode = mode
        return self

    def with_edit_mode_provider(self, provider: EditModeProvider) -> Self:
        if provider.id in self._edit_mode_providers:
            raise WorkspaceConfigurationError(f'Duplicate workspace edit mode provider: {provider.id}')
        try:
            EditModeId(provider.id)
        except ValueError as error:
            raise WorkspaceConfigurationError(str(error)) from error
        self._edit_mode_providers[provider.id] = provider
        return self

    def build(self) -> NativeWorkspaceSession:
        if self._built:
            raise WorkspaceConfigurationError('Workspace builder has already built a session')
        self._built = True
        search, ast, fff = self._selected_providers()
        rootless = self._root is None
        root, cleanup = self._control_root()
        return NativeWorkspaceSession(
            root=root,
            files_provider=self._files_provider,
            search_provider=search,
            ast_provider=ast,
            fff_provider=fff,
            view_provider=self._view_provider,
            observation_store=self._observation_store,
            edit_mode=self._edit_mode,
            edit_mode_providers=tuple(self._edit_mode_providers.values()),
            policy=self._policy,
            cleanup=cleanup,
            enabled_operations=self._rootless_operations() if rootless else None,
        )

    def _control_root(self) -> tuple[Path, Callable[[], None] | None]:
        if self._root is not None:
            return self._root, None
        if not self._selected:
            raise WorkspaceConfigurationError('Rootless workspace requires explicit providers')
        if 'files' in self._selected and 'observations' not in self._selected:
            raise WorkspaceConfigurationError('Rootless files require an explicit observation store')
        temporary = TemporaryDirectory(prefix='ovid-native-workspace-')
        return Path(temporary.name), temporary.cleanup

    def _rootless_operations(self) -> frozenset[WorkspaceOperation]:
        operations = {WorkspaceOperation(operation) for operation in self._selected}
        if WorkspaceOperation.FILES in operations:
            operations.add(WorkspaceOperation.CHANGE_EVENTS)
        return frozenset(operations)

    def _selected_providers(
        self,
    ) -> tuple[WorkspaceSearchProvider | None, WorkspaceAstProvider | None, WorkspaceFffProvider | None]:
        search = self._search_provider
        ast = self._ast_provider
        fff = self._fff_provider
        if self._view_provider is not None:
            if 'search' in self._native_selected:
                search = NativeViewSearchProvider(self._view_provider)
            if 'ast' in self._native_selected:
                if self._files_provider is None:
                    raise WorkspaceConfigurationError('View-backed native AST requires an explicit files provider')
                ast = NativeViewAstProvider(self._view_provider, self._files_provider)
            if 'fff' in self._native_selected:
                fff = NativeViewFffProvider(self._view_provider)
        return search, ast, fff

    def _select(self, operation: str) -> None:
        if operation in self._selected:
            raise WorkspaceConfigurationError(f'{operation.upper()} provider is already selected')
        self._selected.add(operation)


def _require_methods(provider: object, *, operation: str, methods: tuple[str, ...]) -> None:
    missing = tuple(name for name in methods if not callable(getattr(provider, name, None)))
    if missing:
        names = ', '.join(missing)
        raise WorkspaceConfigurationError(f'{operation.upper()} provider is missing required operations: {names}')
