import asyncio
import os
import stat
import time
from pathlib import Path

import pytest
from pytest_mock import MockerFixture

from ovid_native import _native
from ovid_native.ast import (
    AstConfigurationError,
    AstEngine,
    AstLanguageError,
    AstLimitError,
    AstLimits,
    AstPathError,
    AstPatternError,
    AstProposalExpiredError,
    AstProposalNotFoundError,
    AstProposalStaleError,
    AstRewriteApplyRequest,
    AstRewriteOperation,
    AstRewritePreviewRequest,
    AstScanOptions,
    AstSearchRequest,
    AstWriteError,
)
from ovid_native.ast.engine import _call_native, _translate_native
from ovid_native.ast.errors import AstError


def preview_request(*paths: str) -> AstRewritePreviewRequest:
    return AstRewritePreviewRequest(
        operations=(AstRewriteOperation(pattern='print($A)', replacement='logger.info($A)'),),
        scan=AstScanOptions(paths=paths),
        language='python',
    )


def test_search_is_structural_typed_and_deterministic(tmp_path: Path) -> None:
    (tmp_path / 'b.py').write_text("print('b')\n")
    (tmp_path / 'a.py').write_text("print('a')\n# print('comment')\nvalue = \"print('string')\"\n")
    (tmp_path / 'notes.txt').write_text("print('unsupported')\n")
    (tmp_path / 'broken.py').write_text('def broken(\n')
    engine = AstEngine(root=tmp_path)

    result = asyncio.run(engine.search(AstSearchRequest(pattern='print($A)')))

    assert [match.path for match in result.matches] == ['a.py', 'b.py']
    assert result.total_matches == 2
    assert result.files_searched == 2
    assert result.files_with_matches == 2
    assert result.unsupported_files == 1
    assert result.matches[0].captures[0].text == "'a'"
    assert result.matches[0].range.start.line == 1
    assert result.issues[0].kind == 'parse_error'


def test_preview_apply_once_and_permission_preservation(tmp_path: Path) -> None:
    source = tmp_path / 'sample.py'
    source.write_text("print('x')\n# print('comment')\n")
    source.chmod(0o744)
    engine = AstEngine(root=tmp_path)

    preview = asyncio.run(engine.preview_rewrite(preview_request('sample.py')))
    assert preview.total_replacements == 1
    assert preview.changes[0].before == "print('x')"
    assert preview.changes[0].after == "logger.info('x')"
    assert source.read_text() == "print('x')\n# print('comment')\n"

    result = asyncio.run(engine.apply_rewrite(AstRewriteApplyRequest(proposal_id=preview.proposal_id)))
    assert result.total_replacements == 1
    assert source.read_text() == "logger.info('x')\n# print('comment')\n"
    if os.name != 'nt':
        assert stat.S_IMODE(source.stat().st_mode) == 0o744
    with pytest.raises(AstProposalNotFoundError):
        asyncio.run(engine.apply_rewrite(AstRewriteApplyRequest(proposal_id=preview.proposal_id)))


def test_stale_preview_rejects_every_file_before_write(tmp_path: Path) -> None:
    first = tmp_path / 'first.py'
    second = tmp_path / 'second.py'
    first.write_text('print(1)\n')
    second.write_text('print(2)\n')
    engine = AstEngine(root=tmp_path)
    preview = asyncio.run(engine.preview_rewrite(preview_request('.')))
    second.write_text('print(3)\n')

    with pytest.raises(AstProposalStaleError):
        asyncio.run(engine.apply_rewrite(AstRewriteApplyRequest(proposal_id=preview.proposal_id)))
    assert first.read_text() == 'print(1)\n'


def test_zero_change_preview_creates_no_proposal(tmp_path: Path) -> None:
    (tmp_path / 'sample.py').write_text('value = 1\n')
    engine = AstEngine(root=tmp_path)
    preview = asyncio.run(engine.preview_rewrite(preview_request('sample.py')))

    assert preview.proposal_id == ''
    assert preview.total_replacements == 0
    assert asyncio.run(engine.reject_rewrite(preview.proposal_id)) is False
    with pytest.raises(AstProposalNotFoundError):
        asyncio.run(engine.proposal_files('missing'))


def test_proposal_rejection_expiration_and_capacity(tmp_path: Path, mocker: MockerFixture) -> None:
    (tmp_path / 'sample.py').write_text('print(1)\n')
    engine = AstEngine(root=tmp_path, limits=AstLimits(max_pending_proposals=1, proposal_ttl_seconds=1))
    first = asyncio.run(engine.preview_rewrite(preview_request('sample.py')))
    second = asyncio.run(engine.preview_rewrite(preview_request('sample.py')))
    with pytest.raises(AstProposalNotFoundError):
        asyncio.run(engine.apply_rewrite(AstRewriteApplyRequest(proposal_id=first.proposal_id)))
    assert asyncio.run(engine.reject_rewrite(second.proposal_id)) is True
    assert asyncio.run(engine.reject_rewrite(second.proposal_id)) is False

    clock = mocker.patch('ovid_native.ast.engine.time.monotonic', return_value=10.0)
    expiry_engine = AstEngine(root=tmp_path, limits=AstLimits(max_pending_proposals=2, proposal_ttl_seconds=1))
    expired = asyncio.run(expiry_engine.preview_rewrite(preview_request('sample.py')))
    asyncio.run(expiry_engine.preview_rewrite(preview_request('sample.py')))
    clock.return_value = 12.0
    with pytest.raises(AstProposalExpiredError):
        asyncio.run(expiry_engine.apply_rewrite(AstRewriteApplyRequest(proposal_id=expired.proposal_id)))


def test_configuration_and_native_errors_are_narrow(tmp_path: Path) -> None:
    missing = tmp_path / 'missing'
    with pytest.raises(AstConfigurationError):
        AstEngine(root=missing)
    file_root = tmp_path / 'file'
    file_root.write_text('not a directory')
    with pytest.raises(AstConfigurationError):
        AstEngine(root=file_root)

    (tmp_path / 'one.py').write_text('print(1)\n')
    (tmp_path / 'two.py').write_text('print(2)\n')
    engine = AstEngine(root=tmp_path)
    with pytest.raises(AstPathError):
        asyncio.run(engine.search(AstSearchRequest(pattern='print($A)', scan=AstScanOptions(paths=('../x',)))))
    with pytest.raises(AstLanguageError):
        asyncio.run(engine.search(AstSearchRequest(pattern='print($A)', language='unknown')))
    with pytest.raises(AstPatternError):
        asyncio.run(engine.search(AstSearchRequest(pattern='$$$ARGS')))
    limit_engine = AstEngine(root=tmp_path, limits=AstLimits(max_files=1))
    with pytest.raises(AstLimitError):
        asyncio.run(limit_engine.search(AstSearchRequest(pattern='print($A)')))


def test_native_error_translation_covers_every_boundary_type() -> None:
    mappings = (
        (_native.NativeAstConfigurationError('x'), AstConfigurationError),
        (_native.NativeAstPathError('x'), AstPathError),
        (_native.NativeAstLanguageError('x'), AstLanguageError),
        (_native.NativeAstPatternError('x'), AstPatternError),
        (_native.NativeAstLimitError('x'), AstLimitError),
        (_native.NativeAstProposalStaleError('x'), AstProposalStaleError),
        (_native.NativeAstWriteError('x'), AstWriteError),
    )
    for native_error, public_type in mappings:
        translated = _translate_native(native_error)
        assert isinstance(translated, public_type)
        assert str(translated) == 'x'
    assert type(_translate_native(Exception('x'))) is AstError


def test_native_work_runs_off_the_event_loop(tmp_path: Path, mocker: MockerFixture) -> None:
    engine = AstEngine(root=tmp_path)

    def slow_search(root: str, request: _native.NativeAstSearchRequest) -> _native.NativeAstSearchResult:
        del root, request
        time.sleep(0.05)
        return ([], 0, 0, 0, 0, False, [])

    mocker.patch('ovid_native.ast.engine._native.ast_search', side_effect=slow_search)

    async def scenario() -> None:
        task = asyncio.create_task(engine.search(AstSearchRequest(pattern='$A')))
        await asyncio.sleep(0.01)
        assert not task.done()
        await task
        cancelled = asyncio.create_task(engine.search(AstSearchRequest(pattern='$A')))
        await asyncio.sleep(0.01)
        cancelled.cancel()
        with pytest.raises(asyncio.CancelledError):
            await cancelled

    asyncio.run(scenario())


def test_call_native_preserves_public_cause(mocker: MockerFixture) -> None:
    failure = mocker.Mock(side_effect=_native.NativeAstWriteError('write failed'))
    with pytest.raises(AstWriteError) as captured:
        asyncio.run(_call_native(failure))
    assert isinstance(captured.value.__cause__, _native.NativeAstWriteError)

    async def cancel_without_token() -> None:
        task = asyncio.create_task(_call_native(lambda: time.sleep(0.05)))
        await asyncio.sleep(0.01)
        task.cancel()
        with pytest.raises(asyncio.CancelledError):
            await task

    asyncio.run(cancel_without_token())
