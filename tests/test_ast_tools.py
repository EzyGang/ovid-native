import asyncio
from pathlib import Path
from typing import Any, cast

from ovid_core.runtime.context import RunContext
from ovid_core.tools.base import BaseTool, ToolExecutionContext

from ovid_native.ast import (
    AstEditApplyTool,
    AstEditPreviewTool,
    AstRewriteApplyRequest,
    AstRewriteApplyToolResult,
    AstRewriteOperation,
    AstRewritePreviewRequest,
    AstRewritePreviewToolResult,
    AstScanOptions,
    AstSearchRequest,
    AstSearchToolResult,
)
from ovid_native.ast.tools import AstSourceToolset
from ovid_native.workspace.service import NativeWorkspaceSession


def context() -> ToolExecutionContext[None]:
    return cast('ToolExecutionContext[None]', None)


def source_grep(workspace: NativeWorkspaceSession) -> BaseTool[None, Any, Any]:
    toolset = AstSourceToolset[None](
        provider=workspace.ast,
        state=workspace.edit_mode,
        observations=workspace.observations,
    )
    return asyncio.run(toolset.get_tools(cast('RunContext[None]', None)))[0]


def test_tool_contracts_and_approval(tmp_path: Path) -> None:
    workspace = NativeWorkspaceSession(root=tmp_path)
    grep = source_grep(workspace)
    preview = AstEditPreviewTool[None](provider=workspace.ast)
    apply = AstEditApplyTool[None](provider=workspace.ast)

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
    asyncio.run(workspace.close())


def test_tools_execute_search_preview_and_apply(tmp_path: Path) -> None:
    source = tmp_path / 'sample.py'
    source.write_text('print(1)\n')
    workspace = NativeWorkspaceSession(root=tmp_path)
    grep = source_grep(workspace)
    preview_tool = AstEditPreviewTool[None](provider=workspace.ast)
    apply_tool = AstEditApplyTool[None](provider=workspace.ast)

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
    asyncio.run(workspace.close())
