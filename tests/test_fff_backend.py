import asyncio
from pathlib import Path

from ovid_core.services import AgentServices
from pytest_mock import MockerFixture

from ovid_native.fff import FffCapability, FffConfig, FffEngine, select_fff_search_backend
from ovid_native.fff.errors import FffIndexNotReadyError
from ovid_native.search import SearchCapability
from ovid_native.workspace.service import NativeWorkspaceSession, workspace_binding


def test_selects_ready_fff_backend(tmp_path: Path) -> None:
    async def run() -> None:
        (tmp_path / 'sample.py').write_text('value = 1\n')
        fff_engine = FffEngine(root=tmp_path, config=FffConfig(watch=False))
        workspace = NativeWorkspaceSession(root=tmp_path, fff_provider=fff_engine)

        selected = await select_fff_search_backend(workspace=workspace)
        bound = selected.bind(AgentServices((workspace_binding(workspace),)))

        assert isinstance(selected, FffCapability)
        assert [tool.id for tool in bound.contributions.tools] == ['glob', 'find_files']
        assert [toolset.id for toolset in bound.contributions.toolsets] == ['native_fff_source']
        await workspace.close()

    asyncio.run(run())


def test_readiness_failure_selects_native_backend(tmp_path: Path, mocker: MockerFixture) -> None:
    async def run() -> None:
        fff_engine = FffEngine(root=tmp_path, config=FffConfig(watch=False))
        workspace = NativeWorkspaceSession(root=tmp_path, fff_provider=fff_engine)
        mocker.patch.object(fff_engine, 'start', return_value=None)
        mocker.patch.object(fff_engine, 'wait_ready', side_effect=FffIndexNotReadyError('timeout'))
        close = mocker.patch.object(fff_engine, 'close', return_value=None)

        selected = await select_fff_search_backend(workspace=workspace)
        bound = selected.bind(AgentServices((workspace_binding(workspace),)))

        assert isinstance(selected, SearchCapability)
        assert [tool.id for tool in bound.contributions.tools] == ['glob']
        assert [toolset.id for toolset in bound.contributions.toolsets] == ['native_search_source']
        close.assert_not_awaited()
        await workspace.close()
        close.assert_awaited_once()

    asyncio.run(run())
