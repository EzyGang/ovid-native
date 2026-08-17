import asyncio
from pathlib import Path
from typing import Any, cast
from uuid import uuid4

import pytest
from ovid_core.adapters.pydantic_ai import PydanticAIToolsetAdapter
from ovid_core.agents import AgentDefinition, AgentFactory
from ovid_core.config.models import ModelConfig, OvidConfig
from ovid_core.routing.factory import ModelFactory
from ovid_core.routing.models import ModelCapabilities, ModelHandle, ModelRef
from ovid_core.services import AgentServices
from ovid_core.tools import ToolApproval
from pydantic_ai import RunContext as PydanticRunContext
from pydantic_ai.messages import ModelMessage, ModelResponse, TextPart, ToolCallPart, ToolReturnPart
from pydantic_ai.models.function import AgentInfo, FunctionModel
from pydantic_ai.models.test import TestModel
from pydantic_ai.usage import RunUsage
from pytest_mock import MockerFixture

from ovid_native.files import (
    EditMode,
    EditModeToolset,
    ReadLineRange,
    WorkspaceFileReadRequest,
    WorkspaceFilesCapability,
)
from ovid_native.workspace.operations import WorkspaceOperation
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


def upstream_context() -> PydanticRunContext[None]:
    return PydanticRunContext(
        deps=None,
        model=TestModel(),
        usage=RunUsage(),
        tool_call_id='call-1',
        tool_name='apply_patch',
        tool_call_approved=True,
        tool_call_metadata={'approved_by': 'test'},
        run_id=str(uuid4()),
        conversation_id=str(uuid4()),
    )


def test_capability_binds_only_to_complete_workspace_and_requires_approval(tmp_path: Path) -> None:
    workspace = NativeWorkspaceSession(root=tmp_path)
    capability = WorkspaceFilesCapability[None]()
    assert capability.contributions.tools == ()
    assert capability.contributions.toolsets == ()

    bound = capability.bind(AgentServices((workspace_binding(workspace),)))

    assert bound.contributions.instructions
    assert [tool.id for tool in bound.contributions.tools] == ['read', 'write']
    assert bound.contributions.tools[0].approval == ToolApproval()
    assert bound.contributions.tools[1].approval == ToolApproval(
        required=True,
        reason='Create or replace a workspace file',
    )
    assert len(bound.contributions.toolsets) == 1
    asyncio.run(workspace.close())


def test_bound_edit_call_keeps_captured_mode_and_policy_generation(tmp_path: Path) -> None:
    source = tmp_path / 'source.txt'
    source.write_text('one\n')
    workspace = NativeWorkspaceSession(root=tmp_path, edit_mode=EditMode.APPLY_PATCH)
    asyncio.run(
        workspace.files.read_file(WorkspaceFileReadRequest(path='source.txt', ranges=(ReadLineRange(start=1),)))
    )
    capability = WorkspaceFilesCapability[None]().bind(AgentServices((workspace_binding(workspace),)))
    adapter = PydanticAIToolsetAdapter(source=capability.contributions.toolsets[0])
    context = upstream_context()

    first_step = asyncio.run(adapter.for_run_step(context))
    first_definitions = asyncio.run(first_step.get_tools(context))
    assert tuple(first_definitions) == ('edit',)
    assert first_definitions['edit'].tool_def.kind == 'unapproved'
    assert first_definitions['edit'].tool_def.metadata['ovid_input_format'] == 'text'
    workspace.edit_mode.set(EditMode.REPLACE)
    workspace.policy.update(allow_fuzzy_replace=True)
    patch = '*** Begin Patch\n*** Update File: source.txt\n@@\n-one\n+two\n*** End Patch'
    first_result = asyncio.run(
        first_step.call_tool(
            'edit',
            {'input': patch},
            context,
            first_definitions['edit'],
        )
    )
    assert source.read_text() == 'two\n'
    assert first_result['metadata']['mode'] == 'apply_patch'
    assert first_result['metadata']['mode_generation'] == 1
    assert first_result['metadata']['policy_generation'] == 1

    second_step = asyncio.run(adapter.for_run_step(context))
    second_definitions = asyncio.run(second_step.get_tools(context))
    assert tuple(second_definitions) == ('edit',)
    second_result = asyncio.run(
        second_step.call_tool(
            'edit',
            {'path': 'source.txt', 'old_string': 'two', 'new_string': 'three'},
            context,
            second_definitions['edit'],
        )
    )
    assert source.read_text() == 'three\n'
    assert second_result['metadata']['mode'] == 'replace'
    assert second_result['metadata']['mode_generation'] == 2
    assert second_result['metadata']['policy_generation'] == 2
    asyncio.run(workspace.close())


def test_real_agent_sees_mode_schema_change_without_rebuild(tmp_path: Path, mocker: MockerFixture) -> None:
    source = tmp_path / 'source.txt'
    source.write_text('one\n')
    workspace = NativeWorkspaceSession(root=tmp_path, edit_mode=EditMode.APPLY_PATCH)
    seen: list[tuple[str, ...]] = []

    def model(messages: list[ModelMessage], info: AgentInfo) -> ModelResponse:
        names = tuple(tool.name for tool in info.function_tools)
        seen.append(names)
        returns = [part for message in messages for part in message.parts if isinstance(part, ToolReturnPart)]
        if not returns:
            assert 'edit' in names
            workspace.edit_mode.set(EditMode.REPLACE)
            return ModelResponse(parts=[ToolCallPart('read', {'path': 'source.txt'})])
        assert 'edit' in names
        return ModelResponse(parts=[TextPart('observed dynamic files')])

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
        capabilities=(WorkspaceFilesCapability[None](),),
        services=AgentServices((workspace_binding(workspace),)),
    )

    async def run() -> str:
        agent = await factory.build(definition)
        result = await agent.run('Read the source.', deps=None)
        assert agent.diagnostics.services[0].consumers == ('native_files',)
        return result.output

    assert asyncio.run(run()) == 'observed dynamic files'
    assert len(seen) == 2
    asyncio.run(workspace.close())


def test_custom_edit_mode_binds_captured_selection_and_requires_approval(
    tmp_path: Path,
    mocker: MockerFixture,
) -> None:
    custom_tool = mocker.Mock()
    custom_tool.approval = ToolApproval(required=True, reason='Custom workspace mutation')
    provider = mocker.Mock()
    provider.id = 'example.semantic_patch'
    provider.required_operations = frozenset((WorkspaceOperation.FILES, WorkspaceOperation.OBSERVATIONS))
    provider.bind.return_value = custom_tool
    workspace = NativeWorkspaceSession(
        root=tmp_path,
        edit_mode=provider.id,
        edit_mode_providers=(provider,),
    )
    capability = WorkspaceFilesCapability[None]().bind(AgentServices((workspace_binding(workspace),)))
    toolset = capability.contributions.toolsets[0]
    bound = asyncio.run(toolset.for_step(cast('Any', None)))
    tools = asyncio.run(bound.get_tools(cast('Any', None)))

    assert tuple(tools) == (custom_tool,)
    provider.bind.assert_called_once_with(workspace, workspace.edit_mode.current)

    missing_provider = EditModeToolset[None](
        provider=workspace.files,
        state=workspace.edit_mode,
        workspace=workspace,
    )
    with pytest.raises(ValueError, match='provider is unavailable'):
        asyncio.run(missing_provider.for_step(cast('Any', None)))

    missing_workspace = EditModeToolset[None](
        provider=workspace.files,
        state=workspace.edit_mode,
        mode_providers=(provider,),
    )
    with pytest.raises(ValueError, match='requires a workspace session'):
        asyncio.run(missing_workspace.for_step(cast('Any', None)))

    custom_tool.approval = ToolApproval()
    with pytest.raises(ValueError, match='must require approval'):
        asyncio.run(toolset.for_step(cast('Any', None)))

    asyncio.run(workspace.close())
