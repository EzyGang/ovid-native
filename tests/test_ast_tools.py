import asyncio
from pathlib import Path
from typing import cast

from ovid_core.runtime.context import RunContext
from ovid_core.services import AgentServices
from ovid_core.tools.base import ToolExecutionContext

from ovid_native.ast import (
    AstCapability,
    AstEditApplyTool,
    AstEditPreviewTool,
    AstEngine,
    AstGrepTool,
    AstLimits,
    AstRewriteApplyRequest,
    AstRewriteApplyToolResult,
    AstRewriteOperation,
    AstRewritePreviewRequest,
    AstRewritePreviewToolResult,
    AstScanOptions,
    AstSearchRequest,
    AstSearchToolResult,
)
from ovid_native.ast.tools import AST_TOOL_INSTRUCTIONS
from ovid_native.workspace.service import NativeWorkspaceSession, workspace_binding


def context() -> ToolExecutionContext[None]:
    return cast('ToolExecutionContext[None]', None)


def test_capability_contributes_exact_ast_surface(tmp_path: Path) -> None:
    session = NativeWorkspaceSession(root=tmp_path)
    capability = AstCapability[None]().bind(AgentServices((workspace_binding(session),)))
    engine = AstEngine(root=tmp_path)

    assert capability.id == 'native_ast'
    assert capability.description == 'Syntax-aware source search and staged structural rewrites'
    assert capability.defer_loading is True
    assert capability.contributions.instructions == (AST_TOOL_INSTRUCTIONS,)
    assert [tool.id for tool in capability.contributions.tools] == ['ast_edit_preview', 'ast_edit_apply']
    assert [toolset.id for toolset in capability.contributions.toolsets] == ['native_ast_source']
    source_tools = asyncio.run(capability.contributions.toolsets[0].get_tools(cast('RunContext[None]', None)))
    assert [tool.id for tool in source_tools] == ['ast_grep']
    assert engine.root == tmp_path.resolve()
    assert engine.limits == AstLimits()
    second_engine = AstEngine(root=tmp_path)
    assert engine.limits is not second_engine.limits
    asyncio.run(session.close())


def test_tool_contracts_and_approval(tmp_path: Path) -> None:
    engine = AstEngine(root=tmp_path)
    grep = AstGrepTool[None](provider=engine)
    preview = AstEditPreviewTool[None](provider=engine)
    apply = AstEditApplyTool[None](provider=engine)

    assert grep.args_type is AstSearchRequest
    assert grep.result_type is AstSearchToolResult
    assert preview.args_type is AstRewritePreviewRequest
    assert preview.result_type is AstRewritePreviewToolResult
    assert apply.args_type is AstRewriteApplyRequest
    assert apply.result_type is AstRewriteApplyToolResult
    assert all(tool.timeout_seconds == 30.0 for tool in (grep, preview, apply))
    assert all(tool.defer_loading for tool in (grep, preview, apply))
    assert grep.approval.required is False
    assert preview.approval.required is False
    assert apply.approval.required is True
    assert apply.approval.reason == 'Apply a staged structural rewrite to workspace files'


def test_tools_execute_search_preview_and_apply(tmp_path: Path) -> None:
    source = tmp_path / 'sample.py'
    source.write_text('print(1)\n')
    engine = AstEngine(root=tmp_path)
    grep = AstGrepTool[None](provider=engine)
    preview_tool = AstEditPreviewTool[None](provider=engine)
    apply_tool = AstEditApplyTool[None](provider=engine)

    search = asyncio.run(grep.execute(context(), AstSearchRequest(pattern='print($A)')))
    assert search.content['result']['matches'][0]['text'] == 'print(1)'

    preview = asyncio.run(
        preview_tool.execute(
            context(),
            AstRewritePreviewRequest(
                operations=(AstRewriteOperation(pattern='print($A)', replacement='log($A)'),),
                scan=AstScanOptions(paths=('sample.py',)),
                language='python',
            ),
        )
    )
    assert preview.content['preview']['total_replacements'] == 1
    proposal_id = preview.content['preview']['proposal_id']

    applied = asyncio.run(apply_tool.execute(context(), AstRewriteApplyRequest(proposal_id=proposal_id)))
    assert applied.content['result']['files'][0]['path'] == 'sample.py'
    assert source.read_text() == 'log(1)\n'
