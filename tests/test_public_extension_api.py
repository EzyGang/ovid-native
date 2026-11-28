import asyncio
from dataclasses import dataclass, field
from pathlib import Path
from typing import cast

import pytest
from ovid_core.agents import AgentDefinition, AgentFactory
from ovid_core.capabilities.base import CapabilityContributions
from ovid_core.config.models import ModelConfig, OvidConfig
from ovid_core.models import BaseModel
from ovid_core.routing.factory import ModelFactory
from ovid_core.routing.models import ModelCapabilities, ModelHandle, ModelRef
from ovid_core.services import (
    AgentServiceBinding,
    AgentServiceKey,
    AgentServiceMissingError,
    AgentServiceRef,
    AgentServices,
)
from ovid_core.tools.models import ToolResult
from pydantic_ai import ModelMessage, ModelResponse, TextPart, ToolCallPart, ToolReturnPart
from pydantic_ai.models.function import AgentInfo, FunctionModel
from pytest_mock import MockerFixture

from ovid_native.files.models import WorkspaceFileReadRequest
from ovid_native.workspace.builder import WorkspaceSessionBuilder
from ovid_native.workspace.capability import WorkspaceCapability
from ovid_native.workspace.models import WorkspaceSession
from ovid_native.workspace.operations import WORKSPACE_SERVICE_KEY, WorkspaceOperation
from ovid_native.workspace.service import NativeWorkspaceSession, workspace_binding
from ovid_native.workspace.toolsets import WorkspaceResultTool


class RootArgs(BaseModel):
    path: str


class RootContent(BaseModel):
    root: str
    first_line: str


class RootTool(WorkspaceResultTool[None, WorkspaceSession, RootArgs, ToolResult]):
    id = 'root_line'
    description = 'Report the workspace root and the first line of a file.'
    args_type = RootArgs
    result_type = ToolResult

    async def _content(self, arguments: RootArgs) -> BaseModel:
        read = await self._provider.files.read_file(WorkspaceFileReadRequest(path=arguments.path))
        return RootContent(root=str(self._provider.root), first_line=read.lines[0].text)


@dataclass(frozen=True, slots=True, kw_only=True)
class RootCapability(WorkspaceCapability[None]):
    id: str = field(default='root_reader', init=False)
    description: str = field(default='Root reader', init=False)

    def _required_operations(self) -> frozenset[WorkspaceOperation]:
        return frozenset((WorkspaceOperation.FILES,))

    def _contribute(self, session: WorkspaceSession) -> CapabilityContributions[None]:
        return CapabilityContributions(tools=(RootTool(provider=session),))


def handle(model: FunctionModel) -> ModelHandle:
    return ModelHandle(
        model_id='test',
        model_name='function',
        capabilities=ModelCapabilities(
            tools=True, json_schema_output=True, json_object_output=True, image_output=False, thinking=False
        ),
        runtime=model,
    )


def test_public_capability_mounts_and_reads_root_and_files(tmp_path: Path, mocker: MockerFixture) -> None:
    (tmp_path / 'a.txt').write_text('hello\n')
    workspace = NativeWorkspaceSession(root=tmp_path)
    assert workspace.root == tmp_path.resolve()

    def model(messages: list[ModelMessage], info: AgentInfo) -> ModelResponse:
        assert {tool.name for tool in info.function_tools} == {'root_line'}
        returns = [part for message in messages for part in message.parts if isinstance(part, ToolReturnPart)]
        if not returns:
            return ModelResponse(parts=[ToolCallPart('root_line', {'path': 'a.txt'})])
        assert returns[0].content == {'root': str(tmp_path.resolve()), 'first_line': 'hello'}
        return ModelResponse(parts=[TextPart('done')])

    model_factory = mocker.Mock()
    model_factory.build = mocker.AsyncMock(return_value=handle(FunctionModel(model)))
    factory = AgentFactory(
        config=OvidConfig(models={'test': ModelConfig(provider='test', model='function')}),
        model_factory=cast('ModelFactory', model_factory),
    )
    definition = AgentDefinition[None, str](
        model=ModelRef(name='test'),
        deps_type=type(None),
        output_type=str,
        capabilities=(RootCapability(),),
        services=AgentServices((workspace_binding(workspace),)),
    )

    async def run() -> str:
        agent = await factory.build(definition)
        result = await agent.run('Read the root.', deps=None)
        await workspace.close()
        return result.output

    assert asyncio.run(run()) == 'done'


def test_rootless_builder_session_has_no_root(mocker: MockerFixture) -> None:
    session = WorkspaceSessionBuilder().with_search_provider(mocker.Mock()).build()
    assert session.root is None
    asyncio.run(session.close())


def test_capability_rejects_binding_with_older_service_version(tmp_path: Path) -> None:
    workspace = NativeWorkspaceSession(root=tmp_path)
    assert WORKSPACE_SERVICE_KEY.api_version == 4
    old_binding = AgentServiceBinding[WorkspaceSession](
        ref=AgentServiceRef(key=AgentServiceKey(id=WORKSPACE_SERVICE_KEY.id, api_version=3)),
        value=workspace,
        provider='OldSession',
        features=frozenset(operation.value for operation in workspace.operations),
        identity=workspace.id.root,
    )
    with pytest.raises(AgentServiceMissingError, match='is not bound'):
        RootCapability().bind(AgentServices((old_binding,)))
    asyncio.run(workspace.close())
