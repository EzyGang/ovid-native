import asyncio
from pathlib import Path
from typing import cast

import pytest
from ovid_core.agents import AgentDefinition, AgentFactory
from ovid_core.capabilities import BaseCapability
from ovid_core.config.models import ModelConfig, OvidConfig
from ovid_core.routing.factory import ModelFactory
from ovid_core.routing.models import ModelCapabilities, ModelHandle, ModelRef
from ovid_core.services import AgentServices
from pydantic_ai import ModelMessage, ModelResponse, TextPart, ToolCallPart, ToolReturnPart
from pydantic_ai.models.function import AgentInfo, FunctionModel
from pytest_mock import MockerFixture

from ovid_native.ast import AstCapability
from ovid_native.fff import FffCapability
from ovid_native.workspace.service import NativeWorkspaceSession, workspace_binding


_FFF_TOOLS = {'glob', 'find_files', 'grep', 'multi_grep'}
_AST_TOOLS = {'ast_grep', 'ast_edit_preview', 'ast_edit_apply'}


def _first_request_tools(
    tmp_path: Path, mocker: MockerFixture, capability: BaseCapability[None], first_call: ToolCallPart | None
) -> tuple[set[str], list[ToolReturnPart]]:
    (tmp_path / 'notes.txt').write_text('needle\n')
    workspace = NativeWorkspaceSession(root=tmp_path)
    seen: list[set[str]] = []
    returns: list[ToolReturnPart] = []

    def model(messages: list[ModelMessage], info: AgentInfo) -> ModelResponse:
        seen.append({tool.name for tool in info.function_tools})
        returns.extend(part for message in messages for part in message.parts if isinstance(part, ToolReturnPart))
        if first_call is not None and len(seen) == 1:
            return ModelResponse(parts=[first_call])
        return ModelResponse(parts=[TextPart('done')])

    handle = ModelHandle(
        model_id='test',
        model_name='function',
        capabilities=ModelCapabilities(
            tools=True, json_schema_output=True, json_object_output=True, image_output=False, thinking=False
        ),
        runtime=FunctionModel(model),
    )
    model_factory = mocker.Mock()
    model_factory.build = mocker.AsyncMock(return_value=handle)
    factory = AgentFactory(
        config=OvidConfig(models={'test': ModelConfig(provider='test', model='function')}),
        model_factory=cast('ModelFactory', model_factory),
    )
    definition = AgentDefinition[None, str](
        model=ModelRef(name='test'),
        deps_type=type(None),
        output_type=str,
        capabilities=(capability,),
        services=AgentServices((workspace_binding(workspace),)),
    )

    async def run() -> None:
        agent = await factory.build(definition)
        await agent.run('Search the workspace.', deps=None)
        await workspace.close()

    asyncio.run(run())
    return seen[0], returns


def test_eager_fff_tools_are_ready_and_grep_runs_on_the_first_request(tmp_path: Path, mocker: MockerFixture) -> None:
    names, returns = _first_request_tools(
        tmp_path,
        mocker,
        FffCapability[None](include_glob=True, defer_loading=False),
        ToolCallPart('grep', {'query': 'needle', 'mode': 'plain'}, tool_call_id='grep'),
    )

    assert _FFF_TOOLS <= names
    assert 'load_capability' not in names
    assert [part.tool_name for part in returns] == ['grep']
    assert 'notes.txt' in str(returns[0].content)


def test_eager_ast_tools_are_ready(tmp_path: Path, mocker: MockerFixture) -> None:
    names, _ = _first_request_tools(tmp_path, mocker, AstCapability[None](defer_loading=False), None)

    assert _AST_TOOLS <= names
    assert 'load_capability' not in names


@pytest.mark.parametrize(
    'capability',
    [FffCapability[None](include_glob=True), AstCapability[None]()],
    ids=['fff', 'ast'],
)
def test_default_capabilities_hide_every_tool_until_loaded(
    tmp_path: Path, mocker: MockerFixture, capability: BaseCapability[None]
) -> None:
    names, _ = _first_request_tools(tmp_path, mocker, capability, None)

    assert names == {'load_capability'}
