from collections.abc import Sequence
from typing import Any, cast

from ovid_core.runtime.context import RunContext
from ovid_core.tools.base import BaseTool, BaseToolset

from ovid_native.files.edit_modes import EditMode, EditModeProvider, EditModeSelection, EditModeState
from ovid_native.files.edit_tools import ApplyPatchEditTool, HashlineEditTool, PatchEditTool, ReplaceEditTool
from ovid_native.workspace._toolsets import BoundWorkspaceToolset, WorkspaceStepToolset
from ovid_native.workspace.evidence import capture_source_presentation
from ovid_native.workspace.models import WorkspaceFilesProvider, WorkspaceSession


class EditModeToolset[Deps](WorkspaceStepToolset[Deps]):
    id = 'native_files_edit_mode'

    def __init__(
        self,
        *,
        provider: WorkspaceFilesProvider,
        state: EditModeState,
        workspace: WorkspaceSession | None = None,
        mode_providers: Sequence[EditModeProvider] = (),
    ) -> None:
        self._provider = provider
        self._state = state
        self._workspace = workspace
        self._mode_providers = {provider.id: provider for provider in mode_providers}

    async def for_step(self, context: RunContext[Deps]) -> BaseToolset[Deps]:
        del context
        mutation = self._state.capture()
        presentation = capture_source_presentation(mutation.mode, mutation.mode_generation)
        edit_tools: dict[EditMode, BaseTool[Deps, Any, Any]] = {
            EditMode.HASHLINE: HashlineEditTool(
                provider=self._provider,
                mutation=mutation,
                presentation=presentation,
            ),
            EditMode.REPLACE: ReplaceEditTool(
                provider=self._provider,
                mutation=mutation,
                presentation=presentation,
            ),
            EditMode.PATCH: PatchEditTool(
                provider=self._provider,
                mutation=mutation,
                presentation=presentation,
            ),
            EditMode.APPLY_PATCH: ApplyPatchEditTool(
                provider=self._provider,
                mutation=mutation,
                presentation=presentation,
            ),
        }
        selection = EditModeSelection(mode=mutation.mode, generation=mutation.mode_generation)
        edit_tool = (
            edit_tools[EditMode(mutation.mode)]
            if mutation.mode in {mode.value for mode in EditMode}
            else self._custom_tool(mutation.mode, selection)
        )
        tools: tuple[BaseTool[Deps, Any, Any], ...] = (edit_tool,)
        return BoundWorkspaceToolset(owner=self, tools=tools)

    def _custom_tool(
        self,
        mode: str,
        selection: EditModeSelection,
    ) -> BaseTool[Deps, Any, Any]:
        provider = self._mode_providers.get(mode)
        if provider is None:
            raise ValueError(f'Workspace edit mode provider is unavailable: {mode}')
        if self._workspace is None:
            raise ValueError(f'Workspace edit mode provider requires a workspace session: {mode}')
        tool = provider.bind(self._workspace, selection)
        if not tool.approval.required:
            raise ValueError(f'Workspace edit mode must require approval: {mode}')
        return cast(BaseTool[Deps, Any, Any], tool)
