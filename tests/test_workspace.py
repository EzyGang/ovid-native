import asyncio
from pathlib import Path

import pytest
from ovid_core.services import AgentServiceCompatibilityError, AgentServices
from pytest_mock import MockerFixture

from ovid_native.ast import (
    AstProposalNotFoundError,
    AstProposalStaleError,
    AstRewriteApplyRequest,
    AstRewriteOperation,
    AstRewritePreviewRequest,
    AstScanOptions,
    AstSearchRequest,
)
from ovid_native.fff import FffCapability, FffConfigurationError, FffEngine, FffFindRequest
from ovid_native.search import GlobRequest
from ovid_native.workspace.builder import WorkspaceSessionBuilder
from ovid_native.workspace.errors import (
    WorkspaceClosedError,
    WorkspaceConfigurationError,
    WorkspaceOperationUnavailableError,
)
from ovid_native.workspace.models import WorkspaceSessionId
from ovid_native.workspace.operations import WorkspaceOperation, workspace_ref
from ovid_native.workspace.service import NativeWorkspaceSession, workspace_binding
from ovid_native.workspace.stores import NativeObservationStore


def test_session_identity_binding_and_shared_native_handle(tmp_path: Path) -> None:
    first = NativeWorkspaceSession(root=tmp_path)
    second = NativeWorkspaceSession(root=tmp_path)
    binding = workspace_binding(first, name='project')

    assert first.id != second.id
    assert len(first.id.root) >= 22
    assert str(tmp_path) not in first.id.root
    assert binding.ref == workspace_ref('project')
    assert binding.identity == first.id.root
    assert binding.features == frozenset(('files', 'observations', 'change_events', 'search', 'ast', 'fff'))
    assert first.operations == frozenset(
        (
            WorkspaceOperation.FILES,
            WorkspaceOperation.OBSERVATIONS,
            WorkspaceOperation.CHANGE_EVENTS,
            WorkspaceOperation.SEARCH,
            WorkspaceOperation.AST,
            WorkspaceOperation.FFF,
        )
    )
    assert first.search._workspace is first.ast._workspace
    assert first.search._workspace is first.fff._workspace
    retained_ast = first.ast
    retained_fff = first.fff
    assert asyncio.run(retained_fff.status()).state == 'new'

    asyncio.run(first.close())
    with pytest.raises(WorkspaceClosedError):
        asyncio.run(retained_ast.search(AstSearchRequest(pattern='print($A)')))
    with pytest.raises(WorkspaceClosedError):
        asyncio.run(retained_fff.status())
    asyncio.run(second.close())


def test_builder_overrides_one_provider_and_rejects_invalid_choices(tmp_path: Path, mocker: MockerFixture) -> None:
    search = mocker.Mock()
    search.glob = mocker.AsyncMock()
    search.grep = mocker.AsyncMock()
    builder = WorkspaceSessionBuilder.native(root=tmp_path).with_search_provider(search)
    session = builder.build()

    assert session.search is search
    assert session.ast is not None
    assert session.fff is not None

    with pytest.raises(WorkspaceConfigurationError, match='already selected'):
        WorkspaceSessionBuilder.native(root=tmp_path).with_search_provider(search).with_search_provider(search)
    with pytest.raises(WorkspaceConfigurationError, match='missing required operations'):
        WorkspaceSessionBuilder.native(root=tmp_path).with_ast_provider(mocker.Mock(spec=[]))
    ast = mocker.Mock()
    ast.search = mocker.AsyncMock()
    ast.preview_rewrite = mocker.AsyncMock()
    ast.apply_rewrite = mocker.AsyncMock()
    fff = mocker.Mock()
    fff.start = mocker.AsyncMock()
    fff.wait_ready = mocker.AsyncMock()
    fff.find = mocker.AsyncMock()
    fff.grep = mocker.AsyncMock()
    fff.multi_grep = mocker.AsyncMock()
    fff.close = mocker.AsyncMock()
    configured = WorkspaceSessionBuilder.native(root=tmp_path).with_ast_provider(ast).with_fff_provider(fff)
    configured_session = configured.build()
    assert configured_session.ast is ast
    assert configured_session.fff is fff
    files_provider = configured_session.files
    with pytest.raises(WorkspaceConfigurationError, match='AST provider'):
        configured.with_ast_provider(ast)
    with pytest.raises(WorkspaceConfigurationError, match='FFF provider'):
        configured.with_fff_provider(fff)
    asyncio.run(configured_session.close())
    with pytest.raises(WorkspaceConfigurationError, match='already built'):
        builder.build()
    with pytest.raises(WorkspaceConfigurationError):
        NativeWorkspaceSession(root=tmp_path / 'missing')
    with pytest.raises(FffConfigurationError):
        FffEngine(root=tmp_path / 'missing')
    session._operations = frozenset((WorkspaceOperation.SEARCH,))
    with pytest.raises(WorkspaceOperationUnavailableError):
        _ = session.ast

    asyncio.run(session.close())

    with pytest.raises(WorkspaceConfigurationError, match='Rootless workspace requires explicit providers'):
        WorkspaceSessionBuilder().build()
    with pytest.raises(WorkspaceConfigurationError, match='explicit observation store'):
        WorkspaceSessionBuilder().with_files_provider(files_provider).build()
    with pytest.raises(WorkspaceConfigurationError, match='observation store requires a files provider'):
        WorkspaceSessionBuilder().with_search_provider(search).with_observation_store(NativeObservationStore()).build()
    rootless_search = WorkspaceSessionBuilder().with_search_provider(search).build()
    assert rootless_search.operations == frozenset((WorkspaceOperation.SEARCH,))
    asyncio.run(rootless_search.close())
    default_store = NativeWorkspaceSession(root=tmp_path, files_provider=mocker.Mock())
    assert default_store.observations.session_id == default_store.id
    asyncio.run(default_store.close())
    rootless = (
        WorkspaceSessionBuilder()
        .with_files_provider(files_provider)
        .with_search_provider(search)
        .with_observation_store(NativeObservationStore())
        .build()
    )
    assert rootless.operations == frozenset(
        (
            WorkspaceOperation.FILES,
            WorkspaceOperation.CHANGE_EVENTS,
            WorkspaceOperation.SEARCH,
            WorkspaceOperation.OBSERVATIONS,
        )
    )
    with pytest.raises(WorkspaceOperationUnavailableError, match='ast'):
        _ = rootless.ast
    asyncio.run(rootless.close())
    view = mocker.Mock()
    view.acquire_view = mocker.Mock()
    view_session = (
        WorkspaceSessionBuilder.native(root=tmp_path).with_view_provider(view).with_edit_mode('hashline').build()
    )
    assert view_session.view is view
    asyncio.run(view_session.close())
    with pytest.raises(WorkspaceConfigurationError, match='requires an explicit files provider'):
        WorkspaceSessionBuilder.native(root=tmp_path).with_view_provider(view).with_native_ast().build()
    search_view_session = (
        WorkspaceSessionBuilder.native(root=tmp_path).with_view_provider(view).with_native_search().build()
    )
    asyncio.run(search_view_session.close())


def test_builder_validates_custom_edit_mode_names_duplicates_and_operations(
    tmp_path: Path,
    mocker: MockerFixture,
) -> None:
    provider = mocker.Mock()
    provider.id = 'example.semantic_patch'
    provider.required_operations = frozenset((WorkspaceOperation.FILES,))
    duplicate = mocker.Mock()
    duplicate.id = provider.id
    duplicate.required_operations = provider.required_operations
    builder = WorkspaceSessionBuilder.native(root=tmp_path).with_edit_mode_provider(provider)

    with pytest.raises(WorkspaceConfigurationError, match='Duplicate workspace edit mode provider'):
        builder.with_edit_mode_provider(duplicate)

    unnamespaced = mocker.Mock()
    unnamespaced.id = 'semantic_patch'
    unnamespaced.required_operations = frozenset((WorkspaceOperation.FILES,))
    with pytest.raises(WorkspaceConfigurationError, match='globally namespaced'):
        WorkspaceSessionBuilder.native(root=tmp_path).with_edit_mode_provider(unnamespaced)

    unavailable = mocker.Mock()
    unavailable.id = 'example.view_patch'
    unavailable.required_operations = frozenset((WorkspaceOperation.VIEW,))
    with pytest.raises(WorkspaceConfigurationError, match='requires unavailable operations'):
        WorkspaceSessionBuilder.native(root=tmp_path).with_edit_mode_provider(unavailable).build()

    with pytest.raises(WorkspaceConfigurationError, match='Duplicate workspace edit mode provider'):
        NativeWorkspaceSession(root=tmp_path, edit_mode_providers=(provider, duplicate))
    with pytest.raises(WorkspaceConfigurationError, match='globally namespaced'):
        NativeWorkspaceSession(root=tmp_path, edit_mode_providers=(unnamespaced,))

    invalid = mocker.Mock()
    invalid.id = 'example.invalid_patch'
    invalid.required_operations = {'files'}
    with pytest.raises(WorkspaceConfigurationError, match='invalid required operations'):
        NativeWorkspaceSession(root=tmp_path, edit_mode_providers=(invalid,))


def test_close_is_idempotent_closes_native_handle_on_provider_failure(
    tmp_path: Path,
    mocker: MockerFixture,
) -> None:
    fff = mocker.Mock()
    fff.start = mocker.AsyncMock()
    fff.wait_ready = mocker.AsyncMock()
    fff.find = mocker.AsyncMock()
    fff.grep = mocker.AsyncMock()
    fff.multi_grep = mocker.AsyncMock()
    fff.close = mocker.AsyncMock(side_effect=RuntimeError('close failed'))
    session = NativeWorkspaceSession(root=tmp_path, fff_provider=fff)
    search = session.search

    with pytest.raises(RuntimeError, match='close failed'):
        asyncio.run(session.close())

    asyncio.run(session.close())
    fff.close.assert_awaited_once()
    with pytest.raises(WorkspaceClosedError):
        _ = session.search
    with pytest.raises(WorkspaceClosedError):
        asyncio.run(search.glob(GlobRequest(patterns=('.',))))


def test_capability_rejects_workspace_without_required_operation(mocker: MockerFixture) -> None:
    session = mocker.Mock()
    session.id = WorkspaceSessionId('opaque-session-id-123456789')
    session.operations = frozenset((WorkspaceOperation.SEARCH,))
    services = AgentServices((workspace_binding(session),))

    with pytest.raises(AgentServiceCompatibilityError, match='unavailable operations'):
        FffCapability[None]().bind(services)


def test_glob_only_fff_capability_requires_search_only(mocker: MockerFixture) -> None:
    session = mocker.Mock()
    session.id = WorkspaceSessionId('opaque-session-id-123456789')
    session.operations = frozenset((WorkspaceOperation.SEARCH,))
    services = AgentServices((workspace_binding(session),))

    capability = FffCapability[None](
        include_glob=True,
        include_find_files=False,
        include_grep=False,
        include_multi_grep=False,
    ).bind(services)

    assert [tool.id for tool in capability.contributions.tools] == ['glob']


def test_ast_proposals_are_scoped_to_one_session(tmp_path: Path) -> None:
    async def run() -> None:
        source = tmp_path / 'sample.py'
        source.write_text('print(1)\n')
        first = NativeWorkspaceSession(root=tmp_path)
        second = NativeWorkspaceSession(root=tmp_path)
        preview = await first.ast.preview_rewrite(
            AstRewritePreviewRequest(
                operations=(AstRewriteOperation(pattern='print($A)', replacement='logger.info($A)'),),
                scan=AstScanOptions(paths=('sample.py',)),
                language='python',
            )
        )
        stale = await first.ast.preview_rewrite(
            AstRewritePreviewRequest(
                operations=(AstRewriteOperation(pattern='print($A)', replacement='logger.debug($A)'),),
                scan=AstScanOptions(paths=('sample.py',)),
                language='python',
            )
        )
        await first.fff.wait_ready(timeout_seconds=10.0)
        await first.fff.find(FffFindRequest(query='sample'))

        with pytest.raises(AstProposalNotFoundError):
            await second.ast.apply_rewrite(AstRewriteApplyRequest(proposal_id=preview.proposal_id))

        await first.ast.apply_rewrite(AstRewriteApplyRequest(proposal_id=preview.proposal_id))
        with pytest.raises(AstProposalStaleError):
            await first.ast.apply_rewrite(AstRewriteApplyRequest(proposal_id=stale.proposal_id))
        await first.fff.find(FffFindRequest(query='sample'))
        assert source.read_text() == 'logger.info(1)\n'
        await first.close()
        await second.close()

    asyncio.run(run())
