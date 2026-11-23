import asyncio
from pathlib import Path
from typing import cast

from ovid_core.runtime.context import RunContext
from ovid_core.tools.base import ToolExecutionContext

from ovid_native.files.edit_modes import EditMode
from ovid_native.search import GlobRequest, GlobTool, GrepTool, GrepToolRequest, SearchEngine, SearchScanOptions
from ovid_native.search.tools import SearchSourceToolset
from ovid_native.workspace.service import NativeWorkspaceSession


def context() -> ToolExecutionContext[None]:
    return cast('ToolExecutionContext[None]', None)


def test_search_tools_execute_glob_and_auto_mode_grep(tmp_path: Path) -> None:
    (tmp_path / 'sample.py').write_text('value = (\n')
    engine = SearchEngine(root=tmp_path)
    glob = GlobTool[None](provider=engine)
    grep = GrepTool[None](provider=engine)

    discovered = asyncio.run(
        glob.execute(
            context(),
            GlobRequest(patterns=('*.py',), order='path'),
        )
    )
    searched = asyncio.run(
        grep.execute(
            context(),
            GrepToolRequest(pattern='(', scan=SearchScanOptions(paths=('sample.py',))),
        )
    )

    assert discovered.content['result']['matches'][0]['path'] == 'sample.py'
    assert searched.content['result']['interpreted_as_literal'] is True
    assert searched.content['result']['files'][0]['matches'][0]['text'] == '('


def test_source_toolset_keeps_inflight_presentation_and_recaptures_next_step(tmp_path: Path) -> None:
    async def run() -> None:
        (tmp_path / 'sample.py').write_text(f'{"x" * 3_000}\nvalue = 1\nafter\n')
        workspace = NativeWorkspaceSession(root=tmp_path, edit_mode=EditMode.HASHLINE)
        toolset = SearchSourceToolset[None](
            provider=workspace.search,
            state=workspace.edit_mode,
            observations=workspace.observations,
        )
        run_context = cast('RunContext[None]', None)
        assert [tool.id for tool in await toolset.get_tools(run_context)] == ['grep']
        first_step = await toolset.for_step(run_context)
        first_tool = (await first_step.get_tools(run_context))[0]
        workspace.edit_mode.set(EditMode.APPLY_PATCH)
        first = await first_tool.execute(
            context(),
            GrepToolRequest(pattern='value', context_before=1, context_after=1),
        )
        second_step = await toolset.for_step(run_context)
        second_tool = (await second_step.get_tools(run_context))[0]
        second = await second_tool.execute(context(), GrepToolRequest(pattern='value'))

        assert cast(str, first.content).startswith('[sample.py#')
        assert isinstance(second.content, dict)
        assert second.content['result']['files'][0]['path'] == 'sample.py'
        await workspace.close()

    asyncio.run(run())
