import asyncio
from pathlib import Path
from typing import cast

from ovid_core.services import AgentServices
from ovid_core.tools.base import ToolExecutionContext

from ovid_native.search import (
    GlobRequest,
    GlobTool,
    GlobToolResult,
    GrepTool,
    GrepToolRequest,
    GrepToolResult,
    SearchCapability,
    SearchEngine,
    SearchScanOptions,
)
from ovid_native.search.tools import SEARCH_TOOL_INSTRUCTIONS
from ovid_native.workspace import NativeWorkspaceSession, workspace_binding


def context() -> ToolExecutionContext[None]:
    return cast('ToolExecutionContext[None]', None)


def test_capability_contributes_exact_search_surface(tmp_path: Path) -> None:
    workspace = NativeWorkspaceSession(root=tmp_path)
    capability = SearchCapability[None]()
    bound = capability.bind(AgentServices((workspace_binding(workspace),)))

    assert capability.id == 'native_search'
    assert capability.description == 'Fast workspace path discovery and bounded text search'
    assert capability.defer_loading is False
    assert capability.contributions.tools == ()
    assert bound.contributions.instructions == (SEARCH_TOOL_INSTRUCTIONS,)
    assert [tool.id for tool in bound.contributions.tools] == ['glob', 'grep']
    assert bound.contributions.toolsets == ()

    asyncio.run(workspace.close())


def test_search_tool_contracts_are_essential_reads(tmp_path: Path) -> None:
    engine = SearchEngine(root=tmp_path)
    glob = GlobTool[None](provider=engine)
    grep = GrepTool[None](provider=engine)

    assert glob.args_type is GlobRequest
    assert glob.result_type is GlobToolResult
    assert grep.args_type is GrepToolRequest
    assert grep.result_type is GrepToolResult
    assert glob.timeout_seconds == 5.0
    assert grep.timeout_seconds == 30.0
    assert glob.defer_loading is False
    assert grep.defer_loading is False
    assert glob.approval.required is False
    assert grep.approval.required is False
    assert 'empty incomplete result does not prove absence' in glob.description
    assert 'embedded ripgrep' in grep.description


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
