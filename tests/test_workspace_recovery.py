import asyncio
import hashlib
from pathlib import Path

import pytest

from ovid_native.files.models import HashlineEditRequest, WorkspaceFileReadRequest
from ovid_native.workspace.errors import WorkspaceClosedError
from ovid_native.workspace.models import WorkspaceRecoveryReport
from ovid_native.workspace.service import NativeWorkspaceSession


def _digest(content: bytes) -> str:
    return hashlib.sha256(content).hexdigest()


def _record_line(*fields: str) -> str:
    return '\t'.join(fields) + '\n'


def _leftover_mutation(root: Path, state: Path) -> str:
    mutation_id = '0000000000000000000000000000002a-1'
    (state / 'blobs').mkdir(parents=True)
    (state / 'mutations').mkdir()
    for content in (b'original\n', b'keep\n', b'x\n'):
        (state / 'blobs' / _digest(content)).write_bytes(content)
    (root / 'a.txt').write_bytes(b'changed\n')
    staging = root / f'.ovid-hashline-{mutation_id}-1'
    staging.mkdir()
    (staging / 'source').write_bytes(b'keep\n')
    (root / 'c.txt').write_bytes(b'external\n')
    original, keep, before, after = (_digest(content) for content in (b'original\n', b'keep\n', b'x\n', b'y\n'))
    record = (
        'ovid-mutation-record 1\n'
        + _record_line('update', 'a.txt', '', original, _digest(b'changed\n'), original, '')
        + _record_line('delete', 'b.txt', '', keep, '', keep, 'staged')
        + _record_line('update', 'c.txt', '', before, after, before, '')
        + 'landed\t0\n'
    )
    (state / 'mutations' / f'{mutation_id}.record').write_text(record, encoding='utf-8', newline='')
    return mutation_id


def test_recover_rolls_back_a_leftover_mutation_and_reports_conflicts(tmp_path: Path) -> None:
    root = tmp_path / 'root'
    state = tmp_path / 'state'
    root.mkdir()
    unrelated = root / '.ovid-hashline-unrelated'
    unrelated.mkdir()
    mutation_id = _leftover_mutation(root, state)

    async def run() -> tuple[WorkspaceRecoveryReport, ...]:
        workspace = NativeWorkspaceSession(root=root, state_dir=state)
        try:
            return await workspace.recover()
        finally:
            await workspace.close()

    reports = asyncio.run(run())

    assert reports == (
        WorkspaceRecoveryReport(mutation_id=mutation_id, restored=('a.txt', 'b.txt'), conflicts=('c.txt',)),
    )
    assert (root / 'a.txt').read_bytes() == b'original\n'
    assert (root / 'b.txt').read_bytes() == b'keep\n'
    assert (root / 'c.txt').read_bytes() == b'external\n'
    assert sorted(path.name for path in root.iterdir()) == ['.ovid-hashline-unrelated', 'a.txt', 'b.txt', 'c.txt']
    assert not any((state / 'mutations').iterdir())
    assert not any((state / 'blobs').iterdir())


@pytest.mark.parametrize('journalled', [True, False])
def test_hashline_edit_leaves_no_journal_and_nothing_to_recover(tmp_path: Path, journalled: bool) -> None:
    root = tmp_path / 'root'
    state = tmp_path / 'state'
    root.mkdir()
    state.mkdir()
    (root / 'notes.txt').write_text('before\n')

    async def run() -> tuple[WorkspaceRecoveryReport, ...]:
        workspace = NativeWorkspaceSession(root=root, edit_mode='hashline', state_dir=state if journalled else None)
        try:
            read = await workspace.files.read_file(WorkspaceFileReadRequest(path='notes.txt'))
            assert read.observation is not None
            locator = f'{read.lines[0].line_number}:{read.lines[0].short_hash}'
            edit = (
                f'*** Begin Patch\n[notes.txt#{read.observation.tag}]\n'
                f'PUT {locator}.={locator}:\n+after\n*** End Patch\n'
            )
            await workspace.files.hashline(HashlineEditRequest(input=edit))
            return await workspace.recover()
        finally:
            await workspace.close()

    assert asyncio.run(run()) == ()
    assert (root / 'notes.txt').read_text() == 'after\n'
    assert sorted(path.name for path in state.iterdir()) == (['blobs', 'mutations'] if journalled else [])
    assert not any(any(directory.iterdir()) for directory in state.iterdir())


def test_recover_rejects_a_closed_session(tmp_path: Path) -> None:
    async def run() -> None:
        workspace = NativeWorkspaceSession(root=tmp_path, state_dir=tmp_path / 'state')
        await workspace.close()
        await workspace.recover()

    with pytest.raises(WorkspaceClosedError):
        asyncio.run(run())
