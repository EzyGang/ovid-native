import asyncio
from pathlib import Path
from typing import cast

from ovid_core.runtime import RunContext
from ovid_core.services import AgentServices
from ovid_core.tools.base import ToolExecutionContext

from ovid_native.files import (
    EditMode,
    EditModeToolset,
    HashlineEditRequest,
    PatchEditEntry,
    PatchEditRequest,
    ReadLineRange,
    ReadTool,
    WorkspaceCreateRequest,
    WorkspaceDeleteRequest,
    WorkspaceFileReadRequest,
    WorkspaceFilesCapability,
    WorkspaceMoveRequest,
    WorkspaceReadRequest,
    WorkspaceWriteRequest,
    WriteTool,
)
from ovid_native.workspace.service import NativeWorkspaceSession, workspace_binding


def tool_context() -> ToolExecutionContext[None]:
    return cast('ToolExecutionContext[None]', None)


def run_context() -> RunContext[None]:
    return cast('RunContext[None]', None)


def test_read_and_write_tools_execute_both_dispatch_branches(tmp_path: Path) -> None:
    workspace = NativeWorkspaceSession(root=tmp_path)
    read_tool = ReadTool[None](provider=workspace.files)
    write_tool = WriteTool[None](provider=workspace.files)

    directory = asyncio.run(read_tool.execute(tool_context(), WorkspaceReadRequest(path='.')))
    assert directory.metadata == {'kind': 'directory', 'path': '.', 'truncated': False}

    created = asyncio.run(
        write_tool.execute(
            tool_context(),
            WorkspaceWriteRequest(path='source.txt', content='one\n'),
        )
    )
    assert created.content.startswith('[source.txt]')
    observed = asyncio.run(workspace.files.read_file(WorkspaceFileReadRequest(path='source.txt')))
    assert observed.observation is not None
    replaced = asyncio.run(
        write_tool.execute(
            tool_context(),
            WorkspaceWriteRequest(
                path='source.txt',
                content='two\n',
                operation='replace',
                expected_observation=observed.observation.tag,
            ),
        )
    )
    assert replaced.metadata['mode'] == 'write'
    assert (tmp_path / 'source.txt').read_text() == 'two\n'
    asyncio.run(workspace.close())


def test_edit_mode_toolset_executes_patch_and_recaptures_each_step(tmp_path: Path) -> None:
    (tmp_path / 'source.txt').write_text('one\n')
    workspace = NativeWorkspaceSession(root=tmp_path, edit_mode=EditMode.PATCH)
    asyncio.run(workspace.files.read_file(WorkspaceFileReadRequest(path='source.txt')))
    toolset = EditModeToolset[None](provider=workspace.files, state=workspace.edit_mode)
    tools = asyncio.run(toolset.get_tools(run_context()))
    assert [tool.id for tool in tools] == ['native_files_patch']

    result = asyncio.run(
        tools[0].execute(
            tool_context(),
            PatchEditRequest(
                path='source.txt',
                edits=(PatchEditEntry(operation='delete'),),
            ),
        )
    )
    assert result.content == 'delete: source.txt'
    bound = asyncio.run(toolset.for_step(run_context()))
    workspace.edit_mode.set(EditMode.REPLACE)
    rebound = asyncio.run(bound.for_step(run_context()))
    assert [tool.id for tool in asyncio.run(rebound.get_tools(run_context()))] == ['native_files_replace']
    asyncio.run(workspace.close())


def test_direct_delete_and_move_use_shared_file_engine(tmp_path: Path) -> None:
    workspace = NativeWorkspaceSession(root=tmp_path)
    created = asyncio.run(workspace.files.create_file(WorkspaceCreateRequest(path='./source.txt', content='one\n')))
    moved = asyncio.run(
        workspace.files.move_file(WorkspaceMoveRequest(path=r'.\source.txt', destination='./moved.txt'))
    )
    assert created.changes[0].path == 'source.txt'
    assert moved.changes[0].path == 'source.txt'
    assert moved.changes[0].destination == 'moved.txt'
    assert not (tmp_path / 'source.txt').exists()
    assert (tmp_path / 'moved.txt').read_text() == 'one\n'

    asyncio.run(workspace.files.read_file(WorkspaceFileReadRequest(path=r'.\moved.txt')))
    deleted = asyncio.run(workspace.files.delete_file(WorkspaceDeleteRequest(path='./moved.txt')))
    assert deleted.changes[0].path == 'moved.txt'
    assert deleted.post_edit_sources == ()
    assert not (tmp_path / 'moved.txt').exists()
    asyncio.run(workspace.close())


def test_files_capability_tool_contracts(tmp_path: Path) -> None:
    workspace = NativeWorkspaceSession(root=tmp_path)
    capability = WorkspaceFilesCapability[None]().bind(AgentServices((workspace_binding(workspace),)))
    read, write = capability.contributions.tools

    assert read.args_type is WorkspaceReadRequest
    assert write.args_type is WorkspaceWriteRequest
    assert read.timeout_seconds == 30.0
    assert write.timeout_seconds == 30.0
    assert read.approval.required is False
    assert write.approval.required is True
    assert write.approval.reason == 'Create or replace a workspace file'
    asyncio.run(workspace.close())


def test_read_and_hashline_tools_render_captured_source_modes(tmp_path: Path) -> None:
    async def run() -> None:
        source = tmp_path / 'source.txt'
        source.write_text('one\ntwo\n')
        workspace = NativeWorkspaceSession(root=tmp_path, edit_mode=EditMode.HASHLINE)
        read_tool = ReadTool[None](provider=workspace.files, state=workspace.edit_mode)
        observed = await read_tool.execute(tool_context(), WorkspaceReadRequest(path='source.txt'))
        header, first, _ = cast(str, observed.content).splitlines()
        assert header.startswith('[source.txt#')

        toolset = EditModeToolset[None](
            provider=workspace.files,
            state=workspace.edit_mode,
            workspace=workspace,
        )
        hashline = (await toolset.get_tools(run_context()))[0]
        locator = first.split('|', maxsplit=1)[0]
        patch = f'*** Begin Patch\n{header}\nPUT {locator}.={locator}:\n+changed\n*** End Patch\n'
        edited = await hashline.execute(tool_context(), HashlineEditRequest(input=patch))
        assert cast(str, edited.content).startswith('[source.txt#')

        workspace.edit_mode.set(EditMode.APPLY_PATCH)
        partial = await read_tool.execute(
            tool_context(),
            WorkspaceReadRequest(path='source.txt', ranges=(ReadLineRange(start=1, end=1),)),
        )
        assert cast(str, partial.content).splitlines() == ['[source.txt]', '1:changed', '[truncated: 1 of 2 lines]']
        await workspace.close()

    asyncio.run(run())
