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

from ovid_native.ast import AstCapability
from ovid_native.fff import FffCapability
from ovid_native.search import SearchCapability
from ovid_native.workspace.service import NativeWorkspaceSession, workspace_binding


def handle(model: FunctionModel) -> ModelHandle:
    return ModelHandle(
        model_id='test',
        model_name='function',
        capabilities=ModelCapabilities(
            tools=True,
            json_schema_output=True,
            json_object_output=True,
            image_output=False,
            thinking=False,
        ),
        runtime=model,
    )


def test_search_capability_runs_through_real_agent_factory(tmp_path: Path, mocker: MockerFixture) -> None:
    (tmp_path / 'a.txt').write_text('needle\nneedle\n')
    (tmp_path / 'b.txt').write_text('needle b\n')
    (tmp_path / 'oversized.txt').write_text(f'needle {"x" * 50}\n')
    (tmp_path / 'binary.txt').write_bytes(b'nee\0dle')
    (tmp_path / '.hidden.txt').write_text('needle hidden\n')
    (tmp_path / '.gitignore').write_text('ignored.txt\n')
    (tmp_path / 'ignored.txt').write_text('needle ignored\n')
    (tmp_path / 'sample.py').write_text("print('needle')\n")
    workspace = NativeWorkspaceSession(root=tmp_path)

    def model(messages: list[ModelMessage], info: AgentInfo) -> ModelResponse:
        tool_names = {tool.name for tool in info.function_tools}

        tool_returns = [part for message in messages for part in message.parts if isinstance(part, ToolReturnPart)]
        assert tool_names - {'load_capability'} == {
            'ast_edit_apply',
            'ast_edit_preview',
            'ast_grep',
            'find_files',
            'glob',
            'grep',
            'multi_grep',
        }
        if not tool_returns:
            return ModelResponse(parts=[ToolCallPart('glob', {'patterns': ['*.txt'], 'order': 'path'})])
        if len(tool_returns) == 1:
            paths = [match['path'] for match in tool_returns[0].content['content']['result']['matches']]
            assert paths == ['a.txt', 'b.txt', 'binary.txt', 'oversized.txt']
            return ModelResponse(
                parts=[
                    ToolCallPart(
                        'grep',
                        {
                            'pattern': 'needle',
                            'scan': {'paths': paths},
                            'file_limit': 2,
                            'matches_per_file': 1,
                            'max_file_bytes': 16,
                        },
                    )
                ]
            )
        if len(tool_returns) == 2:
            result = tool_returns[-1].content['content']['result']
            assert [file['path'] for file in result['files']] == ['a.txt', 'b.txt']
            assert all(len(file['matches']) == 1 for file in result['files'])
            assert result['next_file_offset'] == 2
            return ModelResponse(
                parts=[
                    ToolCallPart(
                        'grep',
                        {
                            'pattern': 'needle',
                            'scan': {'paths': ['a.txt', 'b.txt', 'binary.txt', 'oversized.txt']},
                            'file_offset': 2,
                            'file_limit': 2,
                            'matches_per_file': 1,
                            'max_file_bytes': 16,
                        },
                    )
                ]
            )

        if len(tool_returns) == 3:
            result = tool_returns[-1].content['content']['result']
            assert result['files'][0]['path'] == 'oversized.txt'
            assert result['files'][0]['coverage']['complete'] is False
            assert result['skipped_binary_files'] == 1
            return ModelResponse(
                parts=[
                    ToolCallPart(
                        'ast_grep',
                        {'pattern': 'print($VALUE)', 'language': 'python', 'scan': {'paths': ['sample.py']}},
                    )
                ]
            )
        if len(tool_returns) == 4:
            result = tool_returns[-1].content['content']['result']
            assert result['matches'][0]['path'] == 'sample.py'
            return ModelResponse(parts=[ToolCallPart('find_files', {'query': 'sample'})])

        result = tool_returns[-1].content['content']['result']
        assert result['matches'][0]['path'] == 'sample.py'
        return ModelResponse(parts=[TextPart('searched')])

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
        capabilities=(SearchCapability[None](), AstCapability[None](), FffCapability[None](include_grep=False)),
        services=AgentServices((workspace_binding(workspace),)),
    )

    async def run() -> str:
        agent = await factory.build(definition)
        result = await agent.run('Discover files and search for needle.', deps=None)
        assert agent.diagnostics.services[0].identity == workspace.id.root
        assert agent.diagnostics.services[0].consumers == ('native_search', 'native_ast', 'native_fff')
        await workspace.close()
        return result.output

    assert asyncio.run(run()) == 'searched'


def test_omitting_search_capability_contributes_no_search_tools(tmp_path: Path, mocker: MockerFixture) -> None:
    def model(messages: list[ModelMessage], info: AgentInfo) -> ModelResponse:
        del messages
        assert {tool.name for tool in info.function_tools}.isdisjoint({'glob', 'grep'})
        return ModelResponse(parts=[TextPart('bare')])

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
    )

    async def run() -> str:
        agent = await factory.build(definition)
        result = await agent.run('Do not use native search.', deps=None)
        return result.output

    assert asyncio.run(run()) == 'bare'
