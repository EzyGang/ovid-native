import asyncio
import shlex
import sys
import time
from collections.abc import Callable
from pathlib import Path
from threading import Event
from typing import cast

import pytest
from ovid_core.agents import AgentDefinition, AgentFactory
from ovid_core.capabilities.base import BaseCapability, CapabilityContributions
from ovid_core.config.models import ModelConfig, OvidConfig
from ovid_core.hooks.base import BaseToolHook
from ovid_core.routing.factory import ModelFactory
from ovid_core.routing.models import ModelCapabilities, ModelHandle, ModelRef
from ovid_core.services import AgentServices
from ovid_core.tools.base import BaseTool, ToolExecutionContext
from ovid_core.tools.models import ToolResult
from pydantic_ai import ModelResponse, TextPart, ToolCallPart
from pydantic_ai.models.function import FunctionModel
from pytest_mock import MockerFixture

from ovid_native import _native
from ovid_native.command.models import WorkspaceCommandRequest
from ovid_native.search import SearchCapability
from ovid_native.workspace.models import WorkspaceCommandProvider
from ovid_native.workspace.operations import WorkspaceOperation
from ovid_native.workspace.service import NativeWorkspaceSession, workspace_binding


class CommandTool(BaseTool[None, WorkspaceCommandRequest, ToolResult]):
    id = 'command'
    description = 'Run one workspace command.'
    args_type = WorkspaceCommandRequest
    result_type = ToolResult

    def __init__(self, *, provider: WorkspaceCommandProvider) -> None:
        self._provider = provider

    async def execute(
        self,
        context: ToolExecutionContext[None],
        arguments: WorkspaceCommandRequest,
    ) -> ToolResult:
        del context
        result = await self._provider.execute(arguments)
        return ToolResult(content=result.output)


async def cancel_run_during_tool(
    started: Callable[[], bool],
    *,
    call: ToolCallPart,
    capability: BaseCapability[None],
    workspace: NativeWorkspaceSession,
    mocker: MockerFixture,
) -> None:
    responses = iter((ModelResponse(parts=[call]), ModelResponse(parts=[TextPart('continued')])))
    capabilities = ModelCapabilities(
        tools=True, json_schema_output=True, json_object_output=True, image_output=False, thinking=False
    )
    runtime = FunctionModel(lambda messages, info: next(responses))
    handle = ModelHandle(model_id='test', model_name='function', capabilities=capabilities, runtime=runtime)
    factory = AgentFactory(
        config=OvidConfig(models={'test': ModelConfig(provider='test', model='function')}),
        model_factory=cast('ModelFactory', mocker.Mock(build=mocker.AsyncMock(return_value=handle))),
    )
    hook = BaseToolHook[None]()
    tool_error = mocker.spy(hook, 'on_tool_error')
    definition = AgentDefinition[None, str](
        model=ModelRef(name='test'),
        deps_type=type(None),
        output_type=str,
        capabilities=(capability,),
        hooks=(hook,),
        services=AgentServices((workspace_binding(workspace),)),
    )
    agent = await factory.build(definition)

    task = asyncio.create_task(agent.run('Use the tool.', deps=None))
    async with asyncio.timeout(10):
        while not started():
            await asyncio.sleep(0.01)
    task.cancel()
    with pytest.raises(asyncio.CancelledError):
        await task

    assert len(list(responses)) == 1
    tool_error.assert_not_called()


def test_cancelled_run_stops_native_command_without_tool_result(tmp_path: Path, mocker: MockerFixture) -> None:
    started = tmp_path / 'started'
    finished = tmp_path / 'finished'
    script = (
        'import time; from pathlib import Path; '
        f'Path({started.name!r}).write_text("started"); time.sleep(0.25); Path({finished.name!r}).write_text("done")'
    )

    async def run() -> None:
        workspace = NativeWorkspaceSession(root=tmp_path, enabled_operations=frozenset((WorkspaceOperation.COMMAND,)))
        try:
            await cancel_run_during_tool(
                started.exists,
                call=ToolCallPart('command', {'command': f'{shlex.quote(sys.executable)} -c {shlex.quote(script)}'}),
                capability=BaseCapability[None](
                    id='command',
                    contributions=CapabilityContributions(tools=(CommandTool(provider=workspace.command),)),
                ),
                workspace=workspace,
                mocker=mocker,
            )
            await asyncio.sleep(0.35)
        finally:
            await workspace.close()

    asyncio.run(run())
    assert not finished.exists()


def test_cancelled_run_cancels_native_grep_without_tool_result(tmp_path: Path, mocker: MockerFixture) -> None:
    (tmp_path / 'source.txt').write_text('needle\n')
    started = Event()
    native_grep = mocker.spy(_native, 'search_grep')

    def grep_until_stopped(
        workspace: _native.NativeWorkspace,
        request: _native.NativeGrepRequest,
    ) -> _native.NativeGrepResult:
        started.set()
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            native_grep(workspace, request)
        raise TimeoutError('native grep was not cancelled')

    mocker.patch('ovid_native.search.engine._native.search_grep', side_effect=grep_until_stopped)

    async def run() -> None:
        workspace = NativeWorkspaceSession(root=tmp_path)
        try:
            await cancel_run_during_tool(
                started.is_set,
                call=ToolCallPart('grep', {'pattern': 'needle'}),
                capability=SearchCapability[None](),
                workspace=workspace,
                mocker=mocker,
            )
        finally:
            await workspace.close()

    asyncio.run(run())
    assert isinstance(native_grep.spy_exception, _native.NativeSearchCancelledError)
