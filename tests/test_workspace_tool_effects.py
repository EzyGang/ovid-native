import asyncio
import importlib
import inspect
import pkgutil
from pathlib import Path
from typing import Any, cast

from ovid_core.adapters.pydantic_ai import resolve_effect
from ovid_core.agents import AgentDefinition, AgentFactory
from ovid_core.config.models import ModelConfig, OvidConfig
from ovid_core.routing.factory import ModelFactory
from ovid_core.routing.models import ModelCapabilities, ModelHandle, ModelRef
from ovid_core.services import AgentServices
from ovid_core.tools import ToolEffect
from ovid_core.tools.base import BaseTool
from pydantic_ai import ModelMessage, ModelResponse, TextPart
from pydantic_ai.models.function import AgentInfo, FunctionModel
from pytest_mock import MockerFixture

import ovid_native
from ovid_native.files import WorkspaceFilesCapability
from ovid_native.workspace.service import NativeWorkspaceSession, workspace_binding


READ_SAFE = ToolEffect(kind='read', replay='safe')
WRITE_UNSAFE = ToolEffect(kind='workspace_write', replay='unsafe')


def concrete_native_tools(tool_type: type[BaseTool[Any, Any, Any]]) -> set[tuple[str, str, ToolEffect]]:
    found: set[tuple[str, str, ToolEffect]] = set()
    for subclass in tool_type.__subclasses__():
        found |= concrete_native_tools(subclass)
        if subclass.__module__.startswith('ovid_native.') and not inspect.isabstract(subclass):
            found.add((subclass.id, subclass.__name__, subclass.effect))

    return found


def test_every_concrete_native_tool_declares_its_effect() -> None:
    for module in pkgutil.walk_packages(ovid_native.__path__, 'ovid_native.'):
        importlib.import_module(module.name)

    assert concrete_native_tools(BaseTool) == {
        ('read', 'ReadTool', READ_SAFE),
        ('glob', 'GlobTool', READ_SAFE),
        ('grep', 'GrepTool', READ_SAFE),
        ('find_files', 'FffFindTool', READ_SAFE),
        ('grep', 'FffGrepTool', READ_SAFE),
        ('multi_grep', 'FffMultiGrepTool', READ_SAFE),
        ('ast_grep', 'AstGrepTool', READ_SAFE),
        ('ast_edit_preview', 'AstEditPreviewTool', READ_SAFE),
        ('write', 'WriteTool', WRITE_UNSAFE),
        ('native_files_replace', 'ReplaceEditTool', WRITE_UNSAFE),
        ('native_files_patch', 'PatchEditTool', WRITE_UNSAFE),
        ('native_files_apply_patch', 'ApplyPatchEditTool', WRITE_UNSAFE),
        ('native_files_hashline', 'HashlineEditTool', WRITE_UNSAFE),
        ('ast_edit_apply', 'AstEditApplyTool', WRITE_UNSAFE),
    }


def test_real_agent_tool_definitions_resolve_native_effects(tmp_path: Path, mocker: MockerFixture) -> None:
    workspace = NativeWorkspaceSession(root=tmp_path)
    effects: dict[str, ToolEffect] = {}

    def model(messages: list[ModelMessage], info: AgentInfo) -> ModelResponse:
        del messages
        effects.update((tool.name, resolve_effect(tool)) for tool in info.function_tools)
        return ModelResponse(parts=[TextPart('resolved')])

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
        capabilities=(WorkspaceFilesCapability[None](),),
        services=AgentServices((workspace_binding(workspace),)),
    )

    async def run() -> str:
        agent = await factory.build(definition)
        result = await agent.run('Report the tool effects.', deps=None)
        return result.output

    assert asyncio.run(run()) == 'resolved'
    assert effects['read'] == READ_SAFE
    assert effects['write'] == WRITE_UNSAFE
    asyncio.run(workspace.close())
