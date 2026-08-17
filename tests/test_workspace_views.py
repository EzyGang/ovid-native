import asyncio
from collections.abc import AsyncIterator
from contextlib import asynccontextmanager
from pathlib import Path

import pytest
from pytest_mock import MockerFixture

from ovid_native.ast.errors import (
    AstProposalExpiredError,
    AstProposalNotFoundError,
    AstProposalStaleError,
    AstWriteError,
)
from ovid_native.ast.models import (
    AstLimits,
    AstRewriteApplyRequest,
    AstRewriteOperation,
    AstRewritePreviewRequest,
    AstScanOptions,
    AstSearchRequest,
)
from ovid_native.fff.models import FffFindRequest, FffGrepRequest, FffMultiGrepRequest
from ovid_native.files.models import WorkspaceReadFileResult, WorkspaceTextSerialization
from ovid_native.search.models import GlobRequest, GrepRequest
from ovid_native.workspace.builder import WorkspaceSessionBuilder
from ovid_native.workspace.errors import WorkspaceStaleError
from ovid_native.workspace.models import WorkspaceView, WorkspaceViewPurpose
from ovid_native.workspace.service import NativeWorkspaceSession
from ovid_native.workspace.stores import NativeObservationStore
from ovid_native.workspace.views import NativeViewAstProvider, NativeViewFffProvider, NativeViewSearchProvider


class StableViewProvider:
    def __init__(self, root: Path, *, exclusive: bool = False) -> None:
        self._root = root
        self._exclusive = exclusive
        self.active = 0
        self.revision_suffix = ''

    @asynccontextmanager
    async def acquire_view(self, purpose: WorkspaceViewPurpose) -> AsyncIterator[WorkspaceView]:
        if self._exclusive and self.active:
            raise RuntimeError('concurrent view acquisition')
        self.active += 1
        try:
            yield WorkspaceView(
                root=self._root,
                revision=f'revision:{purpose}{self.revision_suffix}',
                read_only=True,
            )
        finally:
            self.active -= 1

    async def current_revision(self, purpose: WorkspaceViewPurpose) -> str:
        return f'revision:{purpose}{self.revision_suffix}'


def test_native_operations_use_stable_custom_views_and_ast_commits_through_files(tmp_path: Path) -> None:
    async def run() -> None:
        source = tmp_path / 'source.py'
        source.write_text('print(1)\n')
        backing = NativeWorkspaceSession(root=tmp_path)
        views = StableViewProvider(tmp_path)
        workspace = (
            WorkspaceSessionBuilder()
            .with_files_provider(backing.files)
            .with_edit_mode('apply_patch')
            .with_view_provider(views)
            .with_observation_store(NativeObservationStore())
            .with_native_search()
            .with_native_ast()
            .with_native_fff()
            .build()
        )

        glob = await workspace.search.glob(GlobRequest(patterns=('*.py',)))
        assert glob.matches[0].path == 'source.py'
        grep = await workspace.search.grep(GrepRequest(pattern='print'))
        assert grep.files[0].path == 'source.py'
        assert views.active == 0

        ast = await workspace.ast.search(AstSearchRequest(pattern='print($A)'))
        assert ast.total_matches == 1
        preview = await workspace.ast.preview_rewrite(
            AstRewritePreviewRequest(
                operations=(AstRewriteOperation(pattern='print($A)', replacement='write($A)'),),
                scan=AstScanOptions(paths=('source.py',)),
            )
        )
        assert preview.total_replacements == 1
        assert views.active == 0
        applied = await workspace.ast.apply_rewrite(AstRewriteApplyRequest(proposal_id=preview.proposal_id))
        assert applied.files == preview.files
        assert source.read_text() == 'write(1)\n'
        empty = await workspace.ast.preview_rewrite(
            AstRewritePreviewRequest(
                operations=(AstRewriteOperation(pattern='missing($A)', replacement='write($A)'),),
                scan=AstScanOptions(paths=('source.py',)),
            )
        )
        assert empty.proposal_id == ''
        with pytest.raises(AstProposalNotFoundError):
            await workspace.ast.apply_rewrite(AstRewriteApplyRequest(proposal_id='missing'))
        stale_revision = await workspace.ast.preview_rewrite(
            AstRewritePreviewRequest(
                operations=(AstRewriteOperation(pattern='write($A)', replacement='print($A)'),),
                scan=AstScanOptions(paths=('source.py',)),
            )
        )
        views.revision_suffix = ':changed'
        with pytest.raises(AstProposalStaleError, match='stale workspace revision'):
            await workspace.ast.apply_rewrite(AstRewriteApplyRequest(proposal_id=stale_revision.proposal_id))
        views.revision_suffix = ''

        await workspace.fff.start()
        await workspace.fff.wait_ready(timeout_seconds=10)
        await workspace.fff.start()
        fff = await workspace.fff.grep(FffGrepRequest(query='write', mode='plain'))
        assert fff.workspace_revision == f'revision:{WorkspaceViewPurpose.FFF}'
        found = await workspace.fff.find(FffFindRequest(query='source'))
        assert found.matches[0].path == 'source.py'
        assert views.active == 1

        multi = await workspace.fff.multi_grep(FffMultiGrepRequest(patterns=('write',)))
        assert multi.workspace_revision == f'revision:{WorkspaceViewPurpose.FFF}'
        views.revision_suffix = ':changed'
        with pytest.raises(WorkspaceStaleError, match='view revision changed'):
            await workspace.fff.find(FffFindRequest(query='source'))
        views.revision_suffix = ''
        await workspace.close()
        assert views.active == 0
        await backing.close()

    asyncio.run(run())


def test_configured_ast_limits_apply_to_native_and_view_backends(tmp_path: Path) -> None:
    async def run() -> None:
        (tmp_path / 'source.py').write_text('print(1)\nprint(2)\n')
        limits = AstLimits(max_matches=1)
        native = WorkspaceSessionBuilder.native(root=tmp_path, ast_limits=limits).build()
        direct = await native.ast.search(AstSearchRequest(pattern='print($A)'))
        assert direct.total_matches == 1
        assert direct.truncated is True

        selected = WorkspaceSessionBuilder.native(root=tmp_path, ast_limits=limits).with_native_ast().build()
        selected_result = await selected.ast.search(AstSearchRequest(pattern='print($A)'))
        assert selected_result.total_matches == 1
        assert selected_result.truncated is True

        views = StableViewProvider(tmp_path)
        view_backed = (
            WorkspaceSessionBuilder()
            .with_files_provider(native.files)
            .with_view_provider(views)
            .with_observation_store(NativeObservationStore())
            .with_native_ast(limits=limits)
            .build()
        )
        viewed = await view_backed.ast.search(AstSearchRequest(pattern='print($A)'))
        assert viewed.total_matches == 1
        assert viewed.truncated is True

        await view_backed.close()
        await selected.close()
        await native.close()

    asyncio.run(run())


def test_view_backed_ast_uses_configured_retention_limits(tmp_path: Path, mocker: MockerFixture) -> None:
    async def run() -> None:
        source = tmp_path / 'source.py'
        source.write_text('print(1)\n')
        backing = NativeWorkspaceSession(root=tmp_path)
        views = StableViewProvider(tmp_path)
        request = AstRewritePreviewRequest(
            operations=(AstRewriteOperation(pattern='print($A)', replacement='write($A)'),),
            scan=AstScanOptions(paths=('source.py',)),
        )
        bounded = NativeViewAstProvider(
            views,
            backing.files,
            limits=AstLimits(max_pending_proposals=1),
        )
        first = await bounded.preview_rewrite(request)
        second = await bounded.preview_rewrite(request)
        with pytest.raises(AstProposalNotFoundError):
            await bounded.apply_rewrite(AstRewriteApplyRequest(proposal_id=first.proposal_id))
        assert second.proposal_id

        clock = mocker.patch(
            'ovid_native.workspace.views.monotonic',
            side_effect=(0.0, 0.0, 2.0, 2.0, 3.0, 4.0, 4.0, 6.0, 6.0),
        )
        expiring = NativeViewAstProvider(
            views,
            backing.files,
            limits=AstLimits(proposal_ttl_seconds=1),
        )
        purged_preview = await expiring.preview_rewrite(request)
        retained_preview = await expiring.preview_rewrite(request)
        with pytest.raises(AstProposalNotFoundError):
            await expiring.apply_rewrite(AstRewriteApplyRequest(proposal_id=purged_preview.proposal_id))
        assert retained_preview.proposal_id

        expired_preview = await expiring.preview_rewrite(request)
        with pytest.raises(AstProposalExpiredError):
            await expiring.apply_rewrite(AstRewriteApplyRequest(proposal_id=expired_preview.proposal_id))
        assert clock.call_count == 9

        await backing.close()

    asyncio.run(run())


def test_view_backed_ast_requires_apply_patch_mode(tmp_path: Path) -> None:
    async def run() -> None:
        (tmp_path / 'source.py').write_text('print(1)\n')
        backing = NativeWorkspaceSession(root=tmp_path)
        views = StableViewProvider(tmp_path)
        request = AstRewritePreviewRequest(
            operations=(AstRewriteOperation(pattern='print($A)', replacement='write($A)'),),
            scan=AstScanOptions(paths=('source.py',)),
        )

        unbound = NativeViewAstProvider(views, backing.files)
        unbound_preview = await unbound.preview_rewrite(request)
        with pytest.raises(AstWriteError, match='not bound'):
            await unbound.apply_rewrite(AstRewriteApplyRequest(proposal_id=unbound_preview.proposal_id))

        backing.edit_mode.set('replace')
        wrong_mode = NativeViewAstProvider(views, backing.files)
        wrong_mode.bind_edit_mode(backing.edit_mode)
        wrong_mode_preview = await wrong_mode.preview_rewrite(request)
        with pytest.raises(AstWriteError, match='require apply_patch'):
            await wrong_mode.apply_rewrite(AstRewriteApplyRequest(proposal_id=wrong_mode_preview.proposal_id))

        await backing.close()

    asyncio.run(run())


def test_fff_revision_validation_does_not_acquire_another_view(tmp_path: Path) -> None:
    async def run() -> None:
        views = StableViewProvider(tmp_path, exclusive=True)
        provider = NativeViewFffProvider(views)

        await provider.start()
        await provider.wait_ready(timeout_seconds=10)
        await provider.find(FffFindRequest(query='missing'))

        assert views.active == 1
        await provider.close()
        assert views.active == 0

    asyncio.run(run())


def test_fff_start_failure_closes_engine_and_view(tmp_path: Path, mocker: MockerFixture) -> None:
    async def run() -> None:
        views = StableViewProvider(tmp_path)
        engine = mocker.Mock()
        engine.start = mocker.AsyncMock(side_effect=RuntimeError('index failed'))
        engine.close = mocker.AsyncMock()
        mocker.patch('ovid_native.workspace.views.FffEngine', return_value=engine)

        with pytest.raises(RuntimeError, match='index failed'):
            await NativeViewFffProvider(views).start()

        engine.close.assert_awaited_once()
        assert views.active == 0

    asyncio.run(run())


def test_fff_close_releases_view_after_engine_failure(tmp_path: Path, mocker: MockerFixture) -> None:
    async def run() -> None:
        views = StableViewProvider(tmp_path)
        engine = mocker.Mock()
        engine.start = mocker.AsyncMock(return_value=mocker.Mock())
        engine.close = mocker.AsyncMock(side_effect=RuntimeError('close failed'))
        mocker.patch('ovid_native.workspace.views.FffEngine', return_value=engine)
        provider = NativeViewFffProvider(views)

        await provider.start()
        with pytest.raises(RuntimeError, match='close failed'):
            await provider.close()

        assert views.active == 0

    asyncio.run(run())


def test_view_backed_ast_rejects_stale_source_before_commit(tmp_path: Path) -> None:
    async def run() -> None:
        source = tmp_path / 'source.py'
        source.write_text('print(1)\n')
        backing = NativeWorkspaceSession(root=tmp_path)
        views = StableViewProvider(tmp_path)
        workspace = (
            WorkspaceSessionBuilder()
            .with_files_provider(backing.files)
            .with_view_provider(views)
            .with_observation_store(NativeObservationStore())
            .with_native_search()
            .with_native_ast()
            .with_native_fff()
            .build()
        )
        preview = await workspace.ast.preview_rewrite(
            AstRewritePreviewRequest(
                operations=(AstRewriteOperation(pattern='print($A)', replacement='write($A)'),),
                scan=AstScanOptions(paths=('source.py',)),
            )
        )
        source.write_text('print(2)\n')

        with pytest.raises(AstProposalStaleError, match='source.py'):
            await workspace.ast.apply_rewrite(AstRewriteApplyRequest(proposal_id=preview.proposal_id))

        assert source.read_text() == 'print(2)\n'
        assert views.active == 0
        await workspace.close()
        await backing.close()

    asyncio.run(run())


def test_view_adapters_reject_invalid_and_unstarted_views(tmp_path: Path) -> None:
    class InvalidViewProvider:
        def __init__(self, *, root: Path, read_only: bool) -> None:
            self._root = root
            self._read_only = read_only
            self.active = 0

        @asynccontextmanager
        async def acquire_view(self, purpose: WorkspaceViewPurpose) -> AsyncIterator[WorkspaceView]:
            self.active += 1
            try:
                yield WorkspaceView(
                    root=self._root,
                    revision=f'revision:{purpose}',
                    read_only=self._read_only,
                )
            finally:
                self.active -= 1

        async def current_revision(self, purpose: WorkspaceViewPurpose) -> str:
            return f'revision:{purpose}'

    async def run() -> None:
        writable = InvalidViewProvider(root=tmp_path, read_only=False)
        with pytest.raises(WorkspaceStaleError, match='explicitly read-only'):
            await NativeViewSearchProvider(writable).grep(GrepRequest(pattern='anything'))
        assert writable.active == 0
        with pytest.raises(WorkspaceStaleError, match='explicitly read-only'):
            await NativeViewFffProvider(writable).start()
        assert writable.active == 0

        relative = InvalidViewProvider(root=Path('relative'), read_only=True)
        with pytest.raises(WorkspaceStaleError, match='root must be absolute'):
            await NativeViewSearchProvider(relative).glob(GlobRequest(patterns=('*',)))
        assert relative.active == 0

        unstarted = NativeViewFffProvider(StableViewProvider(tmp_path))
        with pytest.raises(WorkspaceStaleError, match='has not started'):
            await unstarted.find(FffFindRequest(query='anything'))
        await unstarted.close()

    asyncio.run(run())


def test_view_ast_rejects_incomplete_files_provider_results(
    tmp_path: Path,
    mocker: MockerFixture,
) -> None:
    async def run() -> None:
        (tmp_path / 'source.py').write_text('print(1)\n')
        files = mocker.Mock()
        files.read_file = mocker.AsyncMock(
            return_value=WorkspaceReadFileResult(
                path='source.py',
                observation=None,
                lines=(),
                total_lines=1,
                complete_presentation=False,
                editable=False,
                total_bytes=9,
                observation_limit=1,
                serialization=WorkspaceTextSerialization(bom=False, line_ending='lf', terminal_newline=True),
            )
        )
        provider = NativeViewAstProvider(StableViewProvider(tmp_path), files)
        preview = await provider.preview_rewrite(
            AstRewritePreviewRequest(
                operations=(AstRewriteOperation(pattern='print($A)', replacement='write($A)'),),
                scan=AstScanOptions(paths=('source.py',)),
            )
        )

        with pytest.raises(AstWriteError, match='requires a complete editable observation'):
            await provider.apply_rewrite(AstRewriteApplyRequest(proposal_id=preview.proposal_id))

    asyncio.run(run())
