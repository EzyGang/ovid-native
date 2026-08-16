import asyncio
from pathlib import Path

import pytest

from ovid_native.files import (
    ApplyPatchEditRequest,
    PatchEditEntry,
    PatchEditRequest,
    ReadLineRange,
    ReplaceEditRequest,
    WorkspaceFileReadRequest,
    WorkspaceObservedLineChangedError,
    WorkspacePatchError,
    WorkspaceUnseenLineError,
)
from ovid_native.workspace.errors import WorkspacePathError
from ovid_native.workspace.policy import WorkspacePolicy
from ovid_native.workspace.service import NativeWorkspaceSession


def observe(workspace: NativeWorkspaceSession, path: str, *ranges: ReadLineRange) -> None:
    asyncio.run(workspace.files.read_file(WorkspaceFileReadRequest(path=path, ranges=tuple(ranges))))


def test_replace_exact_ambiguous_all_and_seen_line_authorization(tmp_path: Path) -> None:
    source = tmp_path / 'source.txt'
    source.write_text('alpha\nbeta\nbeta\n')
    workspace = NativeWorkspaceSession(root=tmp_path)
    observe(workspace, 'source.txt', ReadLineRange(start=1, end=1))

    with pytest.raises(WorkspaceUnseenLineError):
        asyncio.run(
            workspace.files.replace(
                ReplaceEditRequest(path='source.txt', old_string='beta\nbeta', new_string='changed')
            )
        )

    observe(workspace, 'source.txt')
    with pytest.raises(WorkspacePatchError, match='ambiguous'):
        asyncio.run(
            workspace.files.replace(ReplaceEditRequest(path='source.txt', old_string='beta', new_string='changed'))
        )
    result = asyncio.run(
        workspace.files.replace(
            ReplaceEditRequest(
                path='source.txt',
                old_string='beta',
                new_string='changed',
                replace_all=True,
            )
        )
    )
    assert source.read_text() == 'alpha\nchanged\nchanged\n'
    assert result.matching_strategy == 'exact'
    assert result.confidence == 1
    assert result.changes[0].observation is not None
    assert result.post_edit_sources[0].observation.visible_ranges

    with pytest.raises(WorkspacePatchError, match='not found'):
        asyncio.run(
            workspace.files.replace(ReplaceEditRequest(path='source.txt', old_string='missing', new_string='none'))
        )
    asyncio.run(workspace.close())


def test_replace_fuzzy_is_policy_controlled_and_unique(tmp_path: Path) -> None:
    source = tmp_path / 'source.txt'
    source.write_text('load user record\nreturn result\n')
    workspace = NativeWorkspaceSession(
        root=tmp_path,
        policy=WorkspacePolicy(allow_fuzzy_replace=True, fuzzy_replace_threshold=0.8),
    )
    observe(workspace, 'source.txt')

    result = asyncio.run(
        workspace.files.replace(
            ReplaceEditRequest(
                path='source.txt',
                old_string='load usr record',
                new_string='load account record',
            )
        )
    )

    assert source.read_text() == 'load account record\nreturn result\n'
    assert result.matching_strategy == 'fuzzy'
    assert result.confidence is not None and result.confidence >= 0.8
    asyncio.run(workspace.close())


def test_replace_rejects_lines_changed_after_observation_and_preserves_serialization(tmp_path: Path) -> None:
    source = tmp_path / 'source.txt'
    source.write_bytes(b'\xef\xbb\xbfalpha\r\nbeta\r\n')
    workspace = NativeWorkspaceSession(root=tmp_path)
    observe(workspace, 'source.txt')
    source.write_bytes(b'\xef\xbb\xbfchanged\r\nbeta\r\n')

    with pytest.raises(WorkspaceObservedLineChangedError):
        asyncio.run(
            workspace.files.replace(ReplaceEditRequest(path='source.txt', old_string='changed', new_string='updated'))
        )

    observe(workspace, 'source.txt')
    asyncio.run(
        workspace.files.replace(ReplaceEditRequest(path='source.txt', old_string='changed', new_string='updated'))
    )
    assert source.read_bytes() == b'\xef\xbb\xbfupdated\r\nbeta\r\n'
    asyncio.run(workspace.close())


def test_structured_patch_create_update_move_and_delete(tmp_path: Path) -> None:
    source = tmp_path / 'source.txt'
    deleted = tmp_path / 'deleted.txt'
    source.write_text('one\ntwo\n')
    deleted.write_text('remove\n')
    workspace = NativeWorkspaceSession(root=tmp_path)
    observe(workspace, 'source.txt')
    observe(workspace, 'deleted.txt')

    updated = asyncio.run(
        workspace.files.patch(
            PatchEditRequest(
                path='source.txt',
                edits=(PatchEditEntry(operation='update', diff='@@\n one\n-two\n+three'),),
            )
        )
    )
    created = asyncio.run(
        workspace.files.patch(
            PatchEditRequest(
                path='created.txt',
                edits=(PatchEditEntry(operation='create', diff='+created\n+file'),),
            )
        )
    )
    observe(workspace, 'source.txt')
    moved = asyncio.run(
        workspace.files.patch(
            PatchEditRequest(
                path='source.txt',
                edits=(
                    PatchEditEntry(
                        operation='update',
                        diff='@@\n one\n-three\n+moved',
                        destination='moved.txt',
                    ),
                ),
            )
        )
    )
    deleted_result = asyncio.run(
        workspace.files.patch(
            PatchEditRequest(
                path='deleted.txt',
                edits=(PatchEditEntry(operation='delete'),),
            )
        )
    )

    assert not source.exists()
    assert (tmp_path / 'moved.txt').read_text() == 'one\nmoved\n'
    assert (tmp_path / 'created.txt').read_text() == 'created\nfile'
    assert not deleted.exists()
    assert updated.changes[0].operation == 'update'
    assert created.changes[0].operation == 'create'
    assert moved.changes[0].operation == 'move'
    assert deleted_result.changes[0].operation == 'delete'
    asyncio.run(workspace.close())


def test_apply_patch_is_multi_file_ordered_and_fully_preflighted(tmp_path: Path) -> None:
    first = tmp_path / 'first.txt'
    second = tmp_path / 'second.txt'
    first.write_text('first\n')
    second.write_text('second\n')
    workspace = NativeWorkspaceSession(root=tmp_path)
    observe(workspace, 'first.txt')
    observe(workspace, 'second.txt')
    events = []
    subscription = workspace.change_events.subscribe(events.append)

    patch = """*** Begin Patch
*** Update File: first.txt
@@
-first
+updated
*** Add File: added.txt
+added
*** Update File: second.txt
*** Move to: moved.txt
@@
-second
+moved
*** End Patch"""
    result = asyncio.run(workspace.files.apply_patch(ApplyPatchEditRequest(input=patch)))

    assert [change.path for change in result.changes] == ['first.txt', 'added.txt', 'second.txt']
    assert [event.path for event in events] == ['first.txt', 'added.txt', 'second.txt']
    assert first.read_text() == 'updated\n'
    assert (tmp_path / 'added.txt').read_text() == 'added'
    assert not second.exists()
    assert (tmp_path / 'moved.txt').read_text() == 'moved\n'

    invalid = """*** Begin Patch
*** Add File: untouched.txt
+content
*** Update File: missing.txt
@@
-missing
+changed
*** End Patch"""
    with pytest.raises(WorkspacePathError):
        asyncio.run(workspace.files.apply_patch(ApplyPatchEditRequest(input=invalid)))
    assert not (tmp_path / 'untouched.txt').exists()

    with pytest.raises(WorkspacePatchError):
        asyncio.run(workspace.files.apply_patch(ApplyPatchEditRequest(input='invalid')))
    subscription.close()
    asyncio.run(workspace.close())
