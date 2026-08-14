from pathlib import Path
from typing import Self

from ovid_native.workspace.errors import WorkspaceConfigurationError
from ovid_native.workspace.native import NativeWorkspaceSession
from ovid_native.workspace.operations import WorkspaceAstProvider, WorkspaceFffProvider, WorkspaceSearchProvider


class WorkspaceSessionBuilder:
    def __init__(self, *, root: Path) -> None:
        self._root = root
        self._search_provider: WorkspaceSearchProvider | None = None
        self._ast_provider: WorkspaceAstProvider | None = None
        self._fff_provider: WorkspaceFffProvider | None = None
        self._search_selected = False
        self._ast_selected = False
        self._fff_selected = False

    @classmethod
    def native(cls, *, root: Path) -> Self:
        return cls(root=root)

    def with_search_provider(self, provider: WorkspaceSearchProvider) -> Self:
        if self._search_selected:
            raise WorkspaceConfigurationError('Search provider was already selected')

        _validate_provider(provider, operation='search', methods=('glob', 'grep'))
        self._search_provider = provider
        self._search_selected = True
        return self

    def with_ast_provider(self, provider: WorkspaceAstProvider) -> Self:
        if self._ast_selected:
            raise WorkspaceConfigurationError('AST provider was already selected')

        _validate_provider(provider, operation='ast', methods=('search', 'preview_rewrite', 'apply_rewrite'))
        self._ast_provider = provider
        self._ast_selected = True
        return self

    def with_fff_provider(self, provider: WorkspaceFffProvider) -> Self:
        if self._fff_selected:
            raise WorkspaceConfigurationError('FFF provider was already selected')

        _validate_provider(provider, operation='fff', methods=('start', 'find', 'grep', 'multi_grep', 'close'))
        self._fff_provider = provider
        self._fff_selected = True
        return self

    def build(self) -> NativeWorkspaceSession:
        return NativeWorkspaceSession._configured(
            root=self._root,
            search_provider=self._search_provider,
            ast_provider=self._ast_provider,
            fff_provider=self._fff_provider,
        )


def _validate_provider(
    provider: WorkspaceSearchProvider | WorkspaceAstProvider | WorkspaceFffProvider,
    *,
    operation: str,
    methods: tuple[str, ...],
) -> None:
    missing = tuple(method for method in methods if not callable(getattr(provider, method, None)))
    if missing:
        available = tuple(method for method in methods if method not in missing)
        raise WorkspaceConfigurationError(
            f'Workspace {operation} provider is missing operations {missing}; available operations: {available}'
        )
