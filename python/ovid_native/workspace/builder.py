from pathlib import Path
from typing import Self

from ovid_native.files.edit_modes import EditMode
from ovid_native.workspace.errors import WorkspaceConfigurationError
from ovid_native.workspace.models import WorkspaceAstProvider, WorkspaceFffProvider, WorkspaceSearchProvider
from ovid_native.workspace.policy import WorkspacePolicy
from ovid_native.workspace.service import NativeWorkspaceSession


class WorkspaceSessionBuilder:
    def __init__(
        self,
        *,
        root: Path,
        edit_mode: EditMode,
        policy: WorkspacePolicy | None,
    ) -> None:
        self._root = root
        self._edit_mode = edit_mode
        self._policy = policy
        self._search_provider: WorkspaceSearchProvider | None = None
        self._ast_provider: WorkspaceAstProvider | None = None
        self._fff_provider: WorkspaceFffProvider | None = None
        self._search_selected = False
        self._ast_selected = False
        self._fff_selected = False
        self._built = False

    @classmethod
    def native(
        cls,
        *,
        root: Path,
        edit_mode: EditMode = EditMode.APPLY_PATCH,
        policy: WorkspacePolicy | None = None,
    ) -> Self:
        return cls(root=root, edit_mode=edit_mode, policy=policy)

    def with_search_provider(self, provider: WorkspaceSearchProvider) -> Self:
        if self._search_selected:
            raise WorkspaceConfigurationError('Search provider is already selected')

        _require_methods(provider, operation='search', methods=('glob', 'grep'))
        self._search_provider = provider
        self._search_selected = True
        return self

    def with_ast_provider(self, provider: WorkspaceAstProvider) -> Self:
        if self._ast_selected:
            raise WorkspaceConfigurationError('AST provider is already selected')

        _require_methods(provider, operation='ast', methods=('search', 'preview_rewrite', 'apply_rewrite'))
        self._ast_provider = provider
        self._ast_selected = True
        return self

    def with_fff_provider(self, provider: WorkspaceFffProvider) -> Self:
        if self._fff_selected:
            raise WorkspaceConfigurationError('FFF provider is already selected')
        _require_methods(
            provider, operation='fff', methods=('start', 'wait_ready', 'find', 'grep', 'multi_grep', 'close')
        )
        self._fff_provider = provider
        self._fff_selected = True
        return self

    def build(self) -> NativeWorkspaceSession:
        if self._built:
            raise WorkspaceConfigurationError('Workspace builder has already built a session')

        self._built = True
        return NativeWorkspaceSession(
            root=self._root,
            search_provider=self._search_provider,
            ast_provider=self._ast_provider,
            fff_provider=self._fff_provider,
            edit_mode=self._edit_mode,
            policy=self._policy,
        )


def _require_methods(provider: object, *, operation: str, methods: tuple[str, ...]) -> None:
    missing = tuple(name for name in methods if not callable(getattr(provider, name, None)))
    if missing:
        names = ', '.join(missing)
        raise WorkspaceConfigurationError(f'{operation.upper()} provider is missing required operations: {names}')
