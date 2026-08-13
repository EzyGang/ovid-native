import asyncio
from pathlib import Path

from pytest_mock import MockerFixture

from ovid_native.fff import FffCapability, FffConfig, FffEngine, select_fff_search_backend
from ovid_native.fff.errors import FffIndexNotReadyError
from ovid_native.search import SearchCapability, SearchEngine


def test_selects_ready_fff_backend(tmp_path: Path) -> None:
    async def run() -> None:
        (tmp_path / 'sample.py').write_text('value = 1\n')
        fff_engine = FffEngine(root=tmp_path, config=FffConfig(watch=False))
        native_engine = SearchEngine(root=tmp_path)

        selected = await select_fff_search_backend(fff_engine=fff_engine, native_engine=native_engine)

        assert isinstance(selected, FffCapability)
        assert [tool.id for tool in selected.contributions.tools] == ['glob', 'find_files', 'grep', 'multi_grep']
        await fff_engine.close()

    asyncio.run(run())


def test_readiness_failure_selects_native_backend(tmp_path: Path, mocker: MockerFixture) -> None:
    async def run() -> None:
        fff_engine = FffEngine(root=tmp_path, config=FffConfig(watch=False))
        native_engine = SearchEngine(root=tmp_path)
        mocker.patch.object(fff_engine, 'start', return_value=None)
        mocker.patch.object(fff_engine, 'wait_ready', side_effect=FffIndexNotReadyError('timeout'))
        close = mocker.patch.object(fff_engine, 'close', return_value=None)

        selected = await select_fff_search_backend(fff_engine=fff_engine, native_engine=native_engine)

        assert isinstance(selected, SearchCapability)
        assert [tool.id for tool in selected.contributions.tools] == ['glob', 'grep']
        close.assert_awaited_once()

    asyncio.run(run())
