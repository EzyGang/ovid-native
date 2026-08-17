import asyncio
from pathlib import Path

import pytest

from ovid_native.files.models import HashlineEditRequest, ReadLineRange, WorkspaceFileReadRequest
from ovid_native.workspace.errors import WorkspaceUnseenLineError
from ovid_native.workspace.service import NativeWorkspaceSession


def test_hashline_remove_and_edit_then_move_require_complete_current_source(tmp_path: Path) -> None:
    async def run() -> None:
        remove_path = tmp_path / 'remove.txt'
        move_path = tmp_path / 'move.txt'
        remove_path.write_text('one\ntwo\n')
        move_path.write_text('before\n')
        workspace = NativeWorkspaceSession(root=tmp_path, edit_mode='hashline')

        partial = await workspace.files.read_file(
            WorkspaceFileReadRequest(path='remove.txt', ranges=(ReadLineRange(start=1, end=1),))
        )
        assert partial.observation is not None
        remove = f'*** Begin Patch\n[remove.txt#{partial.observation.tag}]\nREM\n*** End Patch\n'
        with pytest.raises(WorkspaceUnseenLineError, match='complete workspace file observation'):
            await workspace.files.hashline(HashlineEditRequest(input=remove))
        assert remove_path.exists()

        complete = await workspace.files.read_file(WorkspaceFileReadRequest(path='remove.txt'))
        assert complete.observation is not None
        remove = f'*** Begin Patch\n[remove.txt#{complete.observation.tag}]\nREM\n*** End Patch\n'
        removed = await workspace.files.hashline(HashlineEditRequest(input=remove))
        assert not remove_path.exists()
        assert removed.changes[0].operation == 'delete'

        observed = await workspace.files.read_file(WorkspaceFileReadRequest(path='./move.txt'))
        assert observed.observation is not None
        locator = f'{observed.lines[0].line_number}:{observed.lines[0].short_hash}'
        move = (
            f'*** Begin Patch\n[./move.txt#{observed.observation.tag}]\n'
            f'PUT {locator}.={locator}:\n+after\nMV ./moved.txt\n*** End Patch\n'
        )
        moved = await workspace.files.hashline(HashlineEditRequest(input=move))
        assert not move_path.exists()
        assert (tmp_path / 'moved.txt').read_text() == 'after\n'
        assert moved.changes[0].path == 'move.txt'
        assert moved.changes[0].destination == 'moved.txt'
        assert moved.post_edit_sources[0].path == 'moved.txt'

        await workspace.close()

    asyncio.run(run())
