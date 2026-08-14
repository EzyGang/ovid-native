import asyncio
from pathlib import Path
from typing import cast

import pytest
from ovid_core.capabilities import BaseCapability
from ovid_core.services import AgentServiceCompatibilityError, AgentServices
from pytest_mock import MockerFixture

import ovid_native.workspace as workspace_module
from ovid_native import _native
from ovid_native.ast import AstCapability
from ovid_native.ast.engine import AstEngine
from ovid_native.fff import FffCapability
from ovid_native.fff.engine import FffEngine
from ovid_native.search import SearchCapability
from ovid_native.search.engine import SearchEngine
from ovid_native.search.errors import SearchConfigurationError
from ovid_native.search.models import GlobRequest
from ovid_native.workspace import (
    NativeWorkspaceSession,
    WorkspaceClosedError,
    WorkspaceConfigurationError,
    WorkspaceOperation,
    WorkspaceOperationUnavailableError,
    WorkspacePathError,
    WorkspaceSessionBuilder,
    WorkspaceSessionId,
    workspace_binding,
    workspace_ref,
)
from ovid_native.workspace.operations import WorkspaceSearchProvider


def test_workspace_module_exposes_lazy_public_contract() -> None:
    assert 'NativeWorkspaceSession' in dir(workspace_module)

    with pytest.raises(AttributeError, match='missing'):
        workspace_module.__getattr__('missing')


def test_session_identity_binding_and_shared_native_handle(tmp_path: Path) -> None:
    first = NativeWorkspaceSession(root=tmp_path)
    second = NativeWorkspaceSession(root=tmp_path)
    search = cast(SearchEngine, first.search)
    ast = cast(AstEngine, first.ast)
    fff = cast(FffEngine, first.fff)
    binding = workspace_binding(first)
    services = AgentServices((binding,))

    assert isinstance(first.id, WorkspaceSessionId)
    assert first.id != second.id
    assert len(first.id.root) >= 22
    assert str(tmp_path) not in first.id.root
    assert first.operations == frozenset(
        {
            WorkspaceOperation.SEARCH,
            WorkspaceOperation.AST,
            WorkspaceOperation.FFF,
        }
    )
    assert search._workspace is ast._workspace is fff._workspace
    assert search._workspace.session_id == first.id.root
    assert services.resolve(workspace_ref()) is first
    assert binding.features == frozenset({'search', 'ast', 'fff'})
    assert binding.identity == first.id.root
    assert str(tmp_path) not in binding.provider

    asyncio.run(first.close())
    asyncio.run(second.close())


def test_builder_supports_partial_override_and_rejects_duplicate_choices(tmp_path: Path) -> None:
    donor = NativeWorkspaceSession(root=tmp_path)
    provider = donor.search
    session = WorkspaceSessionBuilder.native(root=tmp_path).with_search_provider(provider).build()

    assert session.search is provider
    assert isinstance(session.ast, AstEngine)
    assert isinstance(session.fff, FffEngine)

    builder = WorkspaceSessionBuilder.native(root=tmp_path)
    builder.with_search_provider(provider)
    with pytest.raises(WorkspaceConfigurationError, match='already selected'):
        builder.with_search_provider(provider)

    ast_builder = WorkspaceSessionBuilder.native(root=tmp_path).with_ast_provider(donor.ast)
    with pytest.raises(WorkspaceConfigurationError, match='already selected'):
        ast_builder.with_ast_provider(donor.ast)

    fff_builder = WorkspaceSessionBuilder.native(root=tmp_path).with_fff_provider(donor.fff)
    with pytest.raises(WorkspaceConfigurationError, match='already selected'):
        fff_builder.with_fff_provider(donor.fff)

    asyncio.run(session.close())
    asyncio.run(donor.close())


def test_builder_rejects_incompatible_provider_before_build(tmp_path: Path, mocker: MockerFixture) -> None:
    provider = cast(WorkspaceSearchProvider, mocker.Mock(spec=[]))

    with pytest.raises(
        WorkspaceConfigurationError,
        match=r"missing operations .*'glob'.*'grep'.*available operations: \(\)",
    ):
        WorkspaceSessionBuilder.native(root=tmp_path).with_search_provider(provider)


def test_workspace_creation_translates_safe_native_errors(tmp_path: Path, mocker: MockerFixture) -> None:
    with pytest.raises(WorkspaceConfigurationError, match='cannot resolve workspace root'):
        NativeWorkspaceSession(root=tmp_path / 'missing')

    mocker.patch(
        'ovid_native.workspace.native._native.workspace_create',
        side_effect=_native.NativeWorkspacePathError('invalid relative path'),
    )
    with pytest.raises(WorkspacePathError, match='invalid relative path'):
        NativeWorkspaceSession(root=tmp_path)


def test_fff_startup_is_lazy_and_close_is_idempotent(tmp_path: Path) -> None:
    session = NativeWorkspaceSession(root=tmp_path)
    fff = cast(FffEngine, session.fff)

    assert asyncio.run(fff.status()).state == 'new'

    asyncio.run(session.close())
    asyncio.run(session.close())

    with pytest.raises(WorkspaceClosedError, match='closed'):
        _ = session.search


def test_retrieved_provider_rejects_calls_after_session_close(tmp_path: Path) -> None:
    (tmp_path / 'sample.py').write_text('value = 1\n')
    session = NativeWorkspaceSession(root=tmp_path)
    search = cast(SearchEngine, session.search)

    asyncio.run(session.close())

    with pytest.raises(SearchConfigurationError, match='closed'):
        asyncio.run(search.glob(GlobRequest(patterns=('.',))))


def test_unavailable_operation_fails_before_provider_access(tmp_path: Path) -> None:
    session = NativeWorkspaceSession(root=tmp_path)
    session._operations = frozenset({WorkspaceOperation.SEARCH, WorkspaceOperation.FFF})

    with pytest.raises(WorkspaceOperationUnavailableError, match='ast'):
        _ = session.ast

    asyncio.run(session.close())


@pytest.mark.parametrize(
    ('capability', 'operation'),
    (
        (SearchCapability[None](), WorkspaceOperation.SEARCH),
        (AstCapability[None](), WorkspaceOperation.AST),
        (FffCapability[None](), WorkspaceOperation.FFF),
    ),
)
def test_capability_binding_reports_unavailable_workspace_operation(
    tmp_path: Path,
    capability: BaseCapability[None],
    operation: WorkspaceOperation,
) -> None:
    session = NativeWorkspaceSession(root=tmp_path)
    session._operations = session.operations - {operation}
    services = AgentServices((workspace_binding(session),))

    with pytest.raises(
        AgentServiceCompatibilityError,
        match=rf"Capability '{capability.id}'.*\[{operation.value}\].*available features",
    ):
        capability.bind(services)

    asyncio.run(session.close())
