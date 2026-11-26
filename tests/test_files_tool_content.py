import asyncio
from pathlib import Path
from typing import cast

from ovid_core.runtime import RunContext
from ovid_core.tools.base import ToolExecutionContext

from ovid_native.files import (
    ApplyPatchEditRequest,
    EditMode,
    EditModeToolset,
    ReadTool,
    ReplaceEditRequest,
    WorkspaceFileReadRequest,
    WorkspaceMoveRequest,
    WorkspaceReadRequest,
)
from ovid_native.files.edit_tools import tool_edit_result
from ovid_native.workspace.evidence import capture_source_presentation
from ovid_native.workspace.policy import WorkspacePolicy
from ovid_native.workspace.service import NativeWorkspaceSession


def tool_context() -> ToolExecutionContext[None]:
    return cast('ToolExecutionContext[None]', None)


def test_read_content_reports_uneditable_files_and_failed_targets(tmp_path: Path) -> None:
    (tmp_path / 'large.txt').write_bytes(b'one\ntwo\n')
    (tmp_path / 'small.txt').write_bytes(b'ok\n')
    workspace = NativeWorkspaceSession(
        root=tmp_path,
        policy=WorkspacePolicy(max_observation_file_bytes=4),
        edit_mode=EditMode.HASHLINE,
    )
    read_tool = ReadTool[None](provider=workspace.files, state=workspace.edit_mode)

    large = asyncio.run(read_tool.execute(tool_context(), WorkspaceReadRequest(path='large.txt')))
    partial = asyncio.run(read_tool.execute(tool_context(), WorkspaceReadRequest(path='large.txt; missing.txt')))
    complete = asyncio.run(read_tool.execute(tool_context(), WorkspaceReadRequest(path='large.txt; small.txt')))

    assert cast(str, large.content).splitlines() == [
        '[large.txt]',
        '1:one',
        '2:two',
        '[Not editable: 8 bytes exceeds the 4-byte edit limit]',
    ]
    assert large.metadata['editable'] is False
    assert cast(str, partial.content).endswith('\n\n[1 of 2 targets failed]')
    assert partial.metadata['failed_targets'] == 1
    assert 'targets failed' not in cast(str, complete.content)
    asyncio.run(workspace.close())


def test_edit_content_reports_fuzzy_matches_and_unrendered_changes(tmp_path: Path) -> None:
    (tmp_path / 'source.txt').write_text('load user record\nreturn result\n')
    (tmp_path / 'old.txt').write_text('old\n')
    workspace = NativeWorkspaceSession(
        root=tmp_path,
        policy=WorkspacePolicy(allow_fuzzy_replace=True, fuzzy_replace_threshold=0.8),
        edit_mode=EditMode.REPLACE,
    )
    for path in ('source.txt', 'old.txt'):
        asyncio.run(workspace.files.read_file(WorkspaceFileReadRequest(path=path)))
    toolset = EditModeToolset[None](
        provider=workspace.files,
        state=workspace.edit_mode,
        workspace=workspace,
        mode_providers=(),
    )
    replace = asyncio.run(toolset.get_tools(cast('RunContext[None]', None)))[0]

    fuzzy = asyncio.run(
        replace.execute(
            tool_context(),
            ReplaceEditRequest(path='source.txt', old_string='load usr record', new_string='load account record'),
        )
    )
    workspace.edit_mode.set(EditMode.APPLY_PATCH)
    apply_patch = asyncio.run(toolset.get_tools(cast('RunContext[None]', None)))[0]
    patch = '*** Begin Patch\n*** Update File: source.txt\n@@\n-return result\n+return 1\n*** Delete File: old.txt\n'
    patched = asyncio.run(apply_patch.execute(tool_context(), ApplyPatchEditRequest(input=f'{patch}*** End Patch')))

    confidence = fuzzy.metadata['confidence']
    assert fuzzy.metadata['matching_strategy'] == 'fuzzy'
    assert cast(str, fuzzy.content).endswith(
        f'\n\n[Fuzzy match applied with confidence {confidence:.2f}. Check the edited lines]'
    )
    assert cast(str, patched.content).startswith('[source.txt]')
    assert cast(str, patched.content).endswith('\n\ndelete: old.txt')
    asyncio.run(workspace.close())


def test_edit_content_reports_moves_and_incomplete_commits(tmp_path: Path) -> None:
    (tmp_path / 'source.txt').write_text('one\n')
    workspace = NativeWorkspaceSession(root=tmp_path)
    presentation = capture_source_presentation('apply_patch', 1)
    asyncio.run(workspace.files.read_file(WorkspaceFileReadRequest(path='source.txt')))

    moved = asyncio.run(workspace.files.move_file(WorkspaceMoveRequest(path='source.txt', destination='moved.txt')))
    unrendered = moved.model_copy(update={'post_edit_sources': ()})
    incomplete = tool_edit_result(moved.model_copy(update={'commit_complete': False}), presentation)

    assert tool_edit_result(moved, presentation).content == '[moved.txt]\n1:one'
    assert tool_edit_result(unrendered, presentation).content == 'move: source.txt -> moved.txt'
    assert incomplete.content == (
        '[moved.txt]\n1:one\n\n'
        '[Commit incomplete: only the changes shown were written. Reread the files before you retry]'
    )
    assert incomplete.metadata['commit_complete'] is False
    asyncio.run(workspace.close())
