import asyncio
from pathlib import Path
from typing import cast

from ovid_core.agents import AgentDefinition, AgentFactory
from ovid_core.config.models import ModelConfig, OvidConfig
from ovid_core.routing.factory import ModelFactory
from ovid_core.routing.models import ModelCapabilities, ModelHandle, ModelRef
from ovid_core.services import AgentServices
from pydantic_ai import ModelMessage, ModelResponse, TextPart, ToolCallPart, ToolReturnPart
from pydantic_ai.messages import LoadCapabilityCallPart, LoadCapabilityReturnPart
from pydantic_ai.models.function import AgentInfo, FunctionModel
from pydantic_core import to_json
from pytest_mock import MockerFixture

from ovid_native.fff import FffCapability
from ovid_native.files.edit_modes import EditMode
from ovid_native.workspace.service import NativeWorkspaceSession, workspace_binding


def test_hashline_fff_grep_model_input_omits_the_metadata_result(tmp_path: Path, mocker: MockerFixture) -> None:
    for name in ('a.txt', 'b.txt', 'c.txt'):
        (tmp_path / name).write_text('needle\n')
    workspace = NativeWorkspaceSession(root=tmp_path, edit_mode=EditMode.HASHLINE)
    grep_returns: list[ToolReturnPart] = []

    def model(messages: list[ModelMessage], info: AgentInfo) -> ModelResponse:
        del info
        parts = [part for message in messages for part in message.parts]
        if not any(isinstance(part, LoadCapabilityReturnPart) for part in parts):
            return ModelResponse(parts=[LoadCapabilityCallPart(args={'id': 'native_fff'}, tool_call_id='load')])
        returns = [part for part in parts if isinstance(part, ToolReturnPart) and part.tool_name == 'grep']
        if not returns:
            return ModelResponse(parts=[ToolCallPart('grep', {'query': 'needle', 'mode': 'plain', 'limit': 1})])
        grep_returns.extend(returns)
        return ModelResponse(parts=[TextPart('searched')])

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
        capabilities=(FffCapability[None](include_multi_grep=False),),
        services=AgentServices((workspace_binding(workspace),)),
    )

    async def run() -> str:
        await workspace.fff.start()
        await workspace.fff.wait_ready(timeout_seconds=10.0)
        agent = await factory.build(definition)
        result = await agent.run('Search for needle.', deps=None)
        await workspace.close()
        return result.output

    assert asyncio.run(run()) == 'searched'
    (part,) = grep_returns
    sent = part.model_response_str()
    result = to_json(part.metadata['result']).decode()
    summary = sent.split('\n\n')[-1]
    enveloped = to_json({'content': sent.removesuffix(f'\n\n{summary}'), 'metadata': part.metadata}).decode()

    assert sent == part.content
    assert sent.startswith('[')
    assert summary == 'More files may match: continue with file_offset=1.'
    assert part.metadata['result']['next_file_offset'] == 1
    assert result not in sent
    assert '"next_file_offset"' not in sent
    assert len(enveloped) - len(sent) >= len(result) - len(summary)
