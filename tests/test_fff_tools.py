import asyncio
from pathlib import Path
from typing import cast

import pytest
from ovid_core.runtime.context import RunContext
from ovid_core.services import AgentServices
from ovid_core.tools.base import ToolExecutionContext
from pytest_mock import MockerFixture

from ovid_native.fff import (
    FffByteRange,
    FffCapability,
    FffConfigurationError,
    FffGrepMatch,
    FffGrepRequest,
    FffGrepResult,
    FffMultiGrepRequest,
)
from ovid_native.fff.tools import FffSourceToolset
from ovid_native.files.edit_modes import EditMode
from ovid_native.workspace.service import NativeWorkspaceSession, workspace_binding


def context() -> ToolExecutionContext[None]:
    return cast('ToolExecutionContext[None]', None)


def test_source_toolset_captures_plain_context_and_rejects_approximate_hashline(tmp_path: Path) -> None:
    async def run() -> None:
        (tmp_path / 'variants.txt').write_text('before\ncredential_resolver\nafter\na+b\n')
        workspace = NativeWorkspaceSession(root=tmp_path, edit_mode=EditMode.APPLY_PATCH)
        await workspace.fff.start()
        await workspace.fff.wait_ready(timeout_seconds=10.0)
        toolset = FffSourceToolset[None](
            provider=workspace.fff,
            state=workspace.edit_mode,
            observations=workspace.observations,
            include_grep=True,
            include_multi_grep=True,
        )
        run_context = cast('RunContext[None]', None)
        grep, multi = await toolset.get_tools(run_context)
        plain = await grep.execute(
            context(),
            FffGrepRequest(
                query='credential_resolver',
                mode='plain',
                context_before=1,
                context_after=1,
            ),
        )
        literal = await multi.execute(context(), FffMultiGrepRequest(patterns=('a+b',)))
        assert plain.content['result']['matches'][0]['line'] == 'credential_resolver'
        assert literal.content['result']['matches'][0]['line'] == 'a+b'

        workspace.edit_mode.set(EditMode.HASHLINE)
        hashline_grep = (await toolset.get_tools(run_context))[0]
        approximate = await hashline_grep.execute(
            context(),
            FffGrepRequest(query='credential_resolve', mode='fuzzy'),
        )
        assert '[uneditable: FFF match is approximate' in cast(str, approximate.content)
        await workspace.close()

    asyncio.run(run())


def test_exact_provider_match_requires_a_current_absolute_source_span(
    tmp_path: Path,
    mocker: MockerFixture,
) -> None:
    async def run() -> None:
        (tmp_path / 'source.txt').write_bytes(b'\xef\xbb\xbfalpha\r\nbeta\r\n')
        correct = FffGrepMatch(
            path='source.txt',
            line_number=2,
            column=2,
            byte_offset=11,
            line='beta',
            match_ranges=(FffByteRange(start=1, end=3),),
        )
        stale = correct.model_copy(update={'byte_offset': 1})
        missing = correct.model_copy(update={'match_ranges': ()})
        provider = mocker.Mock()
        provider.grep = mocker.AsyncMock(side_effect=(_fff_result(correct), _fff_result(stale), _fff_result(missing)))
        provider.close = mocker.AsyncMock()
        workspace = NativeWorkspaceSession(root=tmp_path, fff_provider=provider, edit_mode=EditMode.HASHLINE)
        toolset = FffSourceToolset[None](
            provider=provider,
            state=workspace.edit_mode,
            observations=workspace.observations,
            include_grep=True,
            include_multi_grep=False,
        )
        grep = (await toolset.get_tools(cast('RunContext[None]', None)))[0]

        current = await grep.execute(context(), FffGrepRequest(query='et'))
        wrong_offset = await grep.execute(context(), FffGrepRequest(query='et'))
        no_span = await grep.execute(context(), FffGrepRequest(query='et'))

        assert cast(str, current.content).splitlines()[1].endswith('|beta')
        assert '[uneditable: source evidence has an invalid UTF-8 span' in cast(str, wrong_offset.content)
        assert '[uneditable: FFF match has no exact source span' in cast(str, no_span.content)
        await workspace.close()

    asyncio.run(run())


def test_hashline_grep_content_reports_fallbacks_incomplete_indexes_and_pagination(
    tmp_path: Path,
    mocker: MockerFixture,
) -> None:
    async def run() -> None:
        (tmp_path / 'source.txt').write_text('beta\n')
        match = FffGrepMatch(
            path='source.txt',
            line_number=1,
            column=1,
            byte_offset=0,
            line='beta',
            match_ranges=(FffByteRange(start=0, end=4),),
        )
        page = _fff_result(match).model_copy(
            update={'completion': 'index_incomplete', 'index_complete': False, 'next_file_offset': 3}
        )
        fallback = page.model_copy(
            update={
                'matches': (),
                'actual_mode': 'fuzzy',
                'fallback_from': 'plain',
                'approximate': True,
                'completion': 'time_budget_reached',
                'index_complete': True,
                'next_file_offset': None,
            }
        )
        provider = mocker.Mock()
        provider.grep = mocker.AsyncMock(side_effect=(page, fallback))
        provider.close = mocker.AsyncMock()
        workspace = NativeWorkspaceSession(root=tmp_path, fff_provider=provider, edit_mode=EditMode.HASHLINE)
        toolset = FffSourceToolset[None](
            provider=provider,
            state=workspace.edit_mode,
            observations=workspace.observations,
            include_grep=True,
            include_multi_grep=False,
        )
        grep = (await toolset.get_tools(cast('RunContext[None]', None)))[0]

        paged = await grep.execute(context(), FffGrepRequest(query='beta'))
        fell_back = await grep.execute(context(), FffGrepRequest(query='btea'))

        assert cast(str, paged.content).split('\n\n')[-1].splitlines() == [
            'Incomplete: the file index is still building, so some files were not searched.',
            'More files may match: continue with file_offset=3.',
        ]
        assert fell_back.content == (
            'No matches (1 files searched).\n'
            'No plain matches, so fuzzy matching was used.\n'
            'Incomplete: the search time budget was reached.'
        )
        await workspace.close()

    asyncio.run(run())


def _fff_result(match: FffGrepMatch) -> FffGrepResult:
    return FffGrepResult(
        matches=(match,),
        actual_mode='plain',
        approximate=False,
        completion='complete',
        indexed_files=1,
        searchable_files=1,
        files_searched=1,
        files_with_matches=1,
        next_file_offset=None,
        index_complete=True,
    )


def test_capability_omits_disabled_tools_and_instructions(tmp_path: Path) -> None:
    workspace = NativeWorkspaceSession(root=tmp_path)
    services = AgentServices((workspace_binding(workspace),))
    capability: FffCapability[None] = FffCapability(
        include_grep=False,
        include_multi_grep=False,
    ).bind(services)

    assert [tool.id for tool in capability.contributions.tools] == ['find_files']
    instructions = capability.contributions.instructions[0]
    assert 'next_file_offset' not in instructions
    assert 'multi_grep' not in instructions
    grep_only: FffCapability[None] = FffCapability(
        include_find_files=False,
        include_multi_grep=False,
    ).bind(services)
    assert grep_only.contributions.tools == ()
    assert [toolset.id for toolset in grep_only.contributions.toolsets] == ['native_fff_source']
    run_context = cast('RunContext[None]', None)
    grep_tools = asyncio.run(grep_only.contributions.toolsets[0].get_tools(run_context))
    assert [tool.id for tool in grep_tools] == ['grep']
    multi_only: FffCapability[None] = FffCapability(
        include_find_files=False,
        include_grep=False,
    ).bind(services)
    multi_tools = asyncio.run(multi_only.contributions.toolsets[0].get_tools(run_context))
    assert [tool.id for tool in multi_tools] == ['multi_grep']
    assert 'find_files' not in grep_only.contributions.instructions[0]
    asyncio.run(workspace.close())


def test_capability_rejects_invalid_configuration() -> None:
    with pytest.raises(FffConfigurationError):
        FffCapability(
            include_glob=False,
            include_find_files=False,
            include_grep=False,
            include_multi_grep=False,
        )
