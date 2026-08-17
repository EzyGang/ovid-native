import asyncio
from pathlib import Path
from typing import cast

import pytest
from ovid_core.runtime.context import RunContext
from ovid_core.services import AgentServices
from ovid_core.tools.base import ToolExecutionContext
from pytest_mock import MockerFixture

from ovid_native.fff import (
    FffCapability,
    FffConfig,
    FffConfigurationError,
    FffEngine,
    FffFindRequest,
    FffFindTool,
    FffGrepMatch,
    FffGrepRequest,
    FffGrepResult,
    FffGrepTool,
    FffMultiGrepRequest,
    FffMultiGrepTool,
)
from ovid_native.fff.tools import FffSourceToolset
from ovid_native.files.edit_modes import EditMode
from ovid_native.workspace.service import NativeWorkspaceSession, workspace_binding


def context() -> ToolExecutionContext[None]:
    return cast('ToolExecutionContext[None]', None)


def test_find_tool_returns_typed_content(tmp_path: Path) -> None:
    async def run() -> None:
        (tmp_path / 'credential_resolver.py').write_text('pass\n')
        async with FffEngine(root=tmp_path, config=FffConfig(watch=False)) as engine:
            tool: FffFindTool[None] = FffFindTool(provider=engine)
            result = await tool.execute(context(), FffFindRequest(query='credentail resolver'))

        assert result.content['result']['matches'][0]['path'] == 'credential_resolver.py'

    asyncio.run(run())


def test_content_tools_return_typed_results(tmp_path: Path) -> None:
    async def run() -> None:
        (tmp_path / 'variants.txt').write_text('credential_resolver\na+b\n')
        async with FffEngine(root=tmp_path, config=FffConfig(watch=False)) as engine:
            grep = await FffGrepTool[None](provider=engine).execute(
                context(),
                FffGrepRequest(query='credential_resolver', mode='plain'),
            )
            multi = await FffMultiGrepTool[None](provider=engine).execute(
                context(),
                FffMultiGrepRequest(patterns=('a+b',)),
            )

        assert grep.content['result']['matches'][0]['line'] == 'credential_resolver'
        assert multi.content['result']['matches'][0]['line'] == 'a+b'

    asyncio.run(run())


def test_source_toolset_captures_plain_context_and_rejects_approximate_hashline(tmp_path: Path) -> None:
    async def run() -> None:
        (tmp_path / 'variants.txt').write_text('before\ncredential_resolver\nafter\n')
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
        assert isinstance(plain.content, dict)
        assert multi.id == 'multi_grep'

        workspace.edit_mode.set(EditMode.HASHLINE)
        hashline_grep = (await toolset.get_tools(run_context))[0]
        approximate = await hashline_grep.execute(
            context(),
            FffGrepRequest(query='credential_resolve', mode='fuzzy'),
        )
        assert '[uneditable: FFF match is approximate' in cast(str, approximate.content)
        await workspace.close()

    asyncio.run(run())


def test_exact_provider_match_without_ranges_remains_editable(
    tmp_path: Path,
    mocker: MockerFixture,
) -> None:
    async def run() -> None:
        (tmp_path / 'source.txt').write_bytes(b'alpha\n')
        provider = mocker.Mock()
        provider.grep = mocker.AsyncMock(
            return_value=FffGrepResult(
                matches=(
                    FffGrepMatch(
                        path='source.txt',
                        line_number=1,
                        column=1,
                        byte_offset=0,
                        line='alpha',
                        match_ranges=(),
                    ),
                ),
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
        )
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
        result = await grep.execute(context(), FffGrepRequest(query='alpha'))

        assert cast(str, result.content).splitlines()[1].endswith('|alpha')
        await workspace.close()

    asyncio.run(run())


def test_capability_contributes_selected_tools(tmp_path: Path) -> None:
    workspace = NativeWorkspaceSession(root=tmp_path)
    capability: FffCapability[None] = FffCapability(include_glob=True).bind(
        AgentServices((workspace_binding(workspace),))
    )

    assert [tool.id for tool in capability.contributions.tools] == ['glob', 'find_files']
    assert [toolset.id for toolset in capability.contributions.toolsets] == ['native_fff_source']
    assert capability.defer_loading
    asyncio.run(workspace.close())


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
