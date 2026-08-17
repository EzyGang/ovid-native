import asyncio
from pathlib import Path
from typing import Any, cast

from ovid_core.runtime.context import RunContext
from ovid_core.services import AgentServices
from ovid_core.tools.base import ToolExecutionContext
from pytest_mock import MockerFixture

from ovid_native.files import HashlineEditRequest, WorkspaceFilesCapability
from ovid_native.search import GrepToolRequest, SearchCapability
from ovid_native.workspace.builder import WorkspaceSessionBuilder
from ovid_native.workspace.operations import WorkspaceOperation
from ovid_native.workspace.service import NativeWorkspaceSession, workspace_binding
from ovid_native.workspace.stores import NativeObservationStore


def test_custom_provider_frontends_share_observations_and_hashline(
    tmp_path: Path,
    mocker: MockerFixture,
) -> None:
    async def run() -> None:
        source = tmp_path / 'remote.py'
        source.write_text('alpha\n')
        backing = NativeWorkspaceSession(root=tmp_path, edit_mode='hashline')
        files = mocker.Mock()
        for method in (
            'read',
            'read_file',
            'list_directory',
            'create_file',
            'replace_file',
            'delete_file',
            'move_file',
            'replace',
            'patch',
            'apply_patch',
            'hashline',
        ):
            setattr(files, method, mocker.AsyncMock(side_effect=getattr(backing.files, method)))
        search = mocker.Mock()
        search.glob = mocker.AsyncMock(side_effect=backing.search.glob)
        search.grep = mocker.AsyncMock(side_effect=backing.search.grep)
        workspace = (
            WorkspaceSessionBuilder()
            .with_files_provider(files)
            .with_search_provider(search)
            .with_observation_store(NativeObservationStore())
            .with_edit_mode('hashline')
            .build()
        )
        services = AgentServices((workspace_binding(workspace),))
        search_capability = SearchCapability[None]().bind(services)
        files_capability = WorkspaceFilesCapability[None]().bind(services)
        run_context = cast('RunContext[None]', None)
        tool_context = cast('ToolExecutionContext[None]', None)
        grep = (await search_capability.contributions.toolsets[0].get_tools(run_context))[0]
        observed = await grep.execute(tool_context, GrepToolRequest(pattern='alpha'))
        header, rendered = cast(str, observed.content).splitlines()
        locator = rendered.split('|', maxsplit=1)[0]
        receipt = await workspace.observations.resolve_observation('remote.py', header.rsplit('#', 1)[1][:-1])
        assert receipt.session_id == workspace.id
        patch = f'*** Begin Patch\n{header}\nPUT {locator}.={locator}:\n+beta\n*** End Patch\n'
        edit = (await files_capability.contributions.toolsets[0].get_tools(run_context))[0]
        await cast(Any, edit).execute(tool_context, HashlineEditRequest(input=patch))

        assert source.read_text() == 'beta\n'
        assert workspace.files is files
        assert workspace.search is search
        assert workspace.operations == frozenset(
            (
                WorkspaceOperation.FILES,
                WorkspaceOperation.OBSERVATIONS,
                WorkspaceOperation.CHANGE_EVENTS,
                WorkspaceOperation.SEARCH,
            )
        )
        await workspace.close()
        await backing.close()

    asyncio.run(run())
