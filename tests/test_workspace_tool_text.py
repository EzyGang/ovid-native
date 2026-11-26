import asyncio
import importlib
import pkgutil
from pathlib import Path
from typing import Any

from ovid_core.services import AgentServices
from ovid_core.tools.base import BaseTool

import ovid_native
from ovid_native.ast.capability import AstCapability
from ovid_native.fff.capability import FffCapability
from ovid_native.files.capability import WorkspaceFilesCapability
from ovid_native.search.capability import SearchCapability
from ovid_native.workspace.service import NativeWorkspaceSession, workspace_binding


def test_model_facing_native_text_never_mentions_lsp(tmp_path: Path) -> None:
    for module in pkgutil.walk_packages(ovid_native.__path__, prefix='ovid_native.'):
        importlib.import_module(module.name)
    tools: list[type[BaseTool[Any, Any, Any]]] = []
    pending: list[type[BaseTool[Any, Any, Any]]] = [BaseTool]
    while pending:
        subclasses = pending.pop().__subclasses__()
        tools.extend(subclasses)
        pending.extend(subclasses)
    descriptions = [getattr(tool, 'description', '') for tool in tools if tool.__module__.startswith('ovid_native.')]
    workspace = NativeWorkspaceSession(root=tmp_path)
    services = AgentServices((workspace_binding(workspace),))
    capabilities = (
        WorkspaceFilesCapability[None](),
        SearchCapability[None](),
        FffCapability[None](include_glob=True),
        AstCapability[None](),
    )
    bound = [capability.bind(services) for capability in capabilities]
    instructions = [text for capability in bound for text in capability.contributions.instructions]
    texts = [*descriptions, *instructions, *(capability.description for capability in bound)]
    asyncio.run(workspace.close())

    assert len(descriptions) >= 10
    assert all('LSP' not in text for text in texts)
