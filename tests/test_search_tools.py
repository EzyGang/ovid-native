import asyncio
from pathlib import Path
from typing import cast

from ovid_core.runtime.context import RunContext
from ovid_core.tools.base import ToolExecutionContext
from pytest_mock import MockerFixture

from ovid_native.files.edit_modes import EditMode
from ovid_native.search import (
    GlobRequest,
    GlobTool,
    GrepContextLine,
    GrepFileCoverage,
    GrepFileMatches,
    GrepMatch,
    GrepPosition,
    GrepRange,
    GrepResult,
    GrepTool,
    GrepToolRequest,
    SearchScanOptions,
)
from ovid_native.search.tools import SearchSourceToolset
from ovid_native.workspace.models import WorkspaceSearchProvider
from ovid_native.workspace.service import NativeWorkspaceSession


def context() -> ToolExecutionContext[None]:
    return cast('ToolExecutionContext[None]', None)


def test_search_tools_execute_glob_and_auto_mode_grep(tmp_path: Path) -> None:
    (tmp_path / 'sample.py').write_text('value = (\n')
    workspace = NativeWorkspaceSession(root=tmp_path)
    glob = GlobTool[None](provider=workspace.search)
    toolset = SearchSourceToolset[None](
        provider=workspace.search,
        state=workspace.edit_mode,
        observations=workspace.observations,
    )
    grep = asyncio.run(toolset.get_tools(cast('RunContext[None]', None)))[0]

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
    asyncio.run(workspace.close())


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


def test_hashline_grep_content_reports_truncation_pagination_skips_and_empty_results(tmp_path: Path) -> None:
    async def run() -> None:
        (tmp_path / 'a.txt').write_text('needle\nneedle\n')
        (tmp_path / 'b.txt').write_text('needle b\n')
        (tmp_path / 'binary.txt').write_bytes(b'nee\0dle')
        (tmp_path / 'oversized.txt').write_bytes(f'needle {"x" * 50}\n'.encode())
        (tmp_path / 'literal.py').write_text('value = (\n')
        workspace = NativeWorkspaceSession(root=tmp_path, edit_mode=EditMode.HASHLINE)
        grep = await _hashline_grep(workspace, workspace.search)
        scan = SearchScanOptions(paths=('a.txt', 'b.txt', 'binary.txt', 'oversized.txt'))
        page = {'pattern': 'needle', 'scan': scan, 'file_limit': 2, 'matches_per_file': 1, 'max_file_bytes': 16}

        first = await grep.execute(context(), GrepToolRequest.model_validate(page))
        second = await grep.execute(context(), GrepToolRequest.model_validate({**page, 'file_offset': 2}))
        literal = await grep.execute(
            context(), GrepToolRequest(pattern='(', scan=SearchScanOptions(paths=('literal.py',)))
        )
        missing = await grep.execute(context(), GrepToolRequest(pattern='absent'))

        assert cast(str, first.content).split('\n\n')[-1].splitlines() == [
            'a.txt: 1 of 2 matches shown.',
            'Skipped files: 1 binary.',
            'More files may match: continue with file_offset=2.',
        ]
        assert first.metadata['result']['next_file_offset'] == 2
        assert cast(str, second.content).split('\n\n')[-1].splitlines() == [
            'oversized.txt: only the first 16 of 58 bytes searched.',
            'Skipped files: 1 binary.',
        ]
        assert cast(str, literal.content).split('\n\n')[-1] == 'The pattern was searched as a literal string.'
        assert missing.content == 'No matches (4 files searched).\nSkipped files: 1 binary.'
        await workspace.close()

    asyncio.run(run())


def test_hashline_grep_content_reports_incomplete_scans_and_empty_pages(tmp_path: Path, mocker: MockerFixture) -> None:
    async def run() -> None:
        (tmp_path / 'a.txt').write_text('needle\n')
        start = GrepPosition(line=1, column=1, byte_offset=0)
        match = GrepMatch(
            text='needle',
            range=GrepRange(start=start, end=GrepPosition(line=1, column=7, byte_offset=6)),
            line_text='needle',
            matched_lines=(GrepContextLine(line_number=1, text='needle'),),
        )
        file = GrepFileMatches(
            path='a.txt',
            matches=(match,),
            total_matches=200,
            matches_truncated=True,
            total_matches_exact=False,
            coverage=GrepFileCoverage(searched_bytes=7, total_bytes=7, complete=True),
        )
        timed_out = GrepResult(
            files=(file,),
            pattern_engine='rust',
            interpreted_as_literal=False,
            completion='deadline_reached',
            files_searched=3,
            files_with_matches=1,
            files_with_matches_exact=False,
            skipped_binary_files=0,
            skipped_encoding_files=1,
            skipped_large_files=2,
            next_file_offset=1,
            truncated=True,
        )
        empty_page = timed_out.model_copy(
            update={
                'files': (),
                'completion': 'file_limit_reached',
                'files_with_matches': 4,
                'skipped_encoding_files': 0,
                'skipped_large_files': 0,
                'next_file_offset': None,
            }
        )
        provider = mocker.Mock()
        provider.grep = mocker.AsyncMock(side_effect=(timed_out, empty_page))
        workspace = NativeWorkspaceSession(root=tmp_path, edit_mode=EditMode.HASHLINE)
        grep = await _hashline_grep(workspace, provider)

        partial = await grep.execute(context(), GrepToolRequest(pattern='needle'))
        past_end = await grep.execute(context(), GrepToolRequest(pattern='needle', file_offset=4))

        assert cast(str, partial.content).split('\n\n')[-1].splitlines() == [
            'a.txt: 1 of 200+ matches shown.',
            'Skipped files: 1 not UTF-8, 2 too large.',
            'Incomplete: the search timed out.',
            'More files may match: continue with file_offset=1.',
        ]
        assert past_end.content == (
            'No matching files at this file_offset (4 in total).\nIncomplete: the scan file limit was reached.'
        )
        await workspace.close()

    asyncio.run(run())


async def _hashline_grep(workspace: NativeWorkspaceSession, provider: WorkspaceSearchProvider) -> GrepTool[None]:
    toolset = SearchSourceToolset[None](
        provider=provider,
        state=workspace.edit_mode,
        observations=workspace.observations,
    )
    return cast('GrepTool[None]', (await toolset.get_tools(cast('RunContext[None]', None)))[0])
