from dataclasses import dataclass, field

from ovid_core.capabilities.base import CapabilityContributions

from ovid_native.files.tools import FILES_TOOL_INSTRUCTIONS, ReadTool, WriteTool
from ovid_native.files.toolset import EditModeToolset
from ovid_native.workspace._capability import WorkspaceCapability
from ovid_native.workspace.models import WorkspaceSession
from ovid_native.workspace.operations import WorkspaceOperation


@dataclass(frozen=True, slots=True, kw_only=True)
class WorkspaceFilesCapability[Deps](WorkspaceCapability[Deps]):
    id: str = field(default='native_files', init=False)
    description: str = field(
        default='Workspace text reading and mode-aware file mutation',
        init=False,
    )
    defer_loading: bool = field(default=False, init=False)

    def _required_operations(self) -> frozenset[WorkspaceOperation]:
        return frozenset((WorkspaceOperation.FILES, WorkspaceOperation.OBSERVATIONS, WorkspaceOperation.CHANGE_EVENTS))

    def _contribute(self, session: WorkspaceSession) -> CapabilityContributions[Deps]:
        provider = session.files
        return CapabilityContributions(
            instructions=(FILES_TOOL_INSTRUCTIONS,),
            tools=(
                ReadTool(provider=provider, state=session.edit_mode),
                WriteTool(provider=provider, state=session.edit_mode),
            ),
            toolsets=(
                EditModeToolset(
                    provider=provider,
                    state=session.edit_mode,
                    workspace=session,
                    mode_providers=session.edit_mode_providers,
                ),
            ),
        )
