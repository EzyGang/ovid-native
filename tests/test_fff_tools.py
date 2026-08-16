import asyncio
from pathlib import Path
from typing import cast

import pytest
from ovid_core.services import AgentServices
from ovid_core.tools.base import ToolExecutionContext

from ovid_native.fff import (
    FffCapability,
    FffConfig,
    FffConfigurationError,
    FffEngine,
    FffFindRequest,
    FffFindTool,
    FffGrepRequest,
    FffGrepTool,
    FffMultiGrepRequest,
    FffMultiGrepTool,
)
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


def test_capability_contributes_selected_tools(tmp_path: Path) -> None:
    workspace = NativeWorkspaceSession(root=tmp_path)
    capability: FffCapability[None] = FffCapability(include_glob=True).bind(
        AgentServices((workspace_binding(workspace),))
    )

    assert [tool.id for tool in capability.contributions.tools] == ['glob', 'find_files', 'grep', 'multi_grep']
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
    assert [tool.id for tool in grep_only.contributions.tools] == ['grep']
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
