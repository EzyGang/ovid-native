import asyncio
from pathlib import Path
from typing import cast

from ovid_core.agents import AgentDefinition, AgentFactory
from ovid_core.config.models import ModelConfig, OvidConfig
from ovid_core.routing.factory import ModelFactory
from ovid_core.routing.models import ModelCapabilities, ModelHandle, ModelRef
from ovid_core.services import AgentServices
from pydantic_ai import ModelMessage, ModelResponse, TextPart, ToolCallPart, ToolReturnPart
from pydantic_ai.models.function import AgentInfo, FunctionModel
from pytest_mock import MockerFixture

from ovid_native.ast import AstCapability, AstEngine, AstRewriteApplyRequest
from ovid_native.workspace import NativeWorkspaceSession, workspace_binding


def test_ast_capability_runs_through_real_agent_factory(tmp_path: Path, mocker: MockerFixture) -> None:
    source = tmp_path / 'sample.py'
    source.write_text("print('x')\n# print('unchanged')\n")
    workspace = NativeWorkspaceSession(root=tmp_path)
    engine = cast(AstEngine, workspace.ast)
    proposal_id = ''

    def model(messages: list[ModelMessage], info: AgentInfo) -> ModelResponse:
        nonlocal proposal_id
        tool_returns = [part for message in messages for part in message.parts if isinstance(part, ToolReturnPart)]
        names = {tool.name for tool in info.function_tools}
        assert {'ast_grep', 'ast_edit_preview', 'ast_edit_apply'} <= names
        if not tool_returns:
            return ModelResponse(parts=[ToolCallPart('ast_grep', {'pattern': 'print($A)', 'language': 'python'})])
        if len(tool_returns) == 1:
            assert tool_returns[0].content['content']['result']['total_matches'] == 1
            return ModelResponse(
                parts=[
                    ToolCallPart(
                        'ast_edit_preview',
                        {
                            'operations': [{'pattern': 'print($A)', 'replacement': 'logger.info($A)'}],
                            'scan': {'paths': ['sample.py']},
                            'language': 'python',
                        },
                    )
                ]
            )
        proposal_id = tool_returns[-1].content['content']['preview']['proposal_id']
        return ModelResponse(parts=[TextPart('previewed')])

    handle = ModelHandle(
        model_id='test',
        model_name='function',
        capabilities=ModelCapabilities(
            tools=True,
            json_schema_output=True,
            json_object_output=True,
            image_output=False,
            thinking=False,
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
        capabilities=(AstCapability[None](),),
        services=AgentServices((workspace_binding(workspace),)),
    )

    async def run() -> str:
        agent = await factory.build(definition)
        result = await agent.run('Search and preview the structural rewrite.', deps=None)
        return result.output

    assert asyncio.run(run()) == 'previewed'
    assert proposal_id
    assert source.read_text() == "print('x')\n# print('unchanged')\n"
    asyncio.run(engine.apply_rewrite(AstRewriteApplyRequest(proposal_id=proposal_id)))
    assert source.read_text() == "logger.info('x')\n# print('unchanged')\n"
    asyncio.run(workspace.close())
