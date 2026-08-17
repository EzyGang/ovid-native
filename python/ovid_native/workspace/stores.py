from ovid_native.workspace.models import WorkspaceFilesProvider, WorkspaceSessionId
from ovid_native.workspace.observations import WorkspaceObservationService
from ovid_native.workspace.provider_observations import ProviderWorkspaceObservationService


class NativeObservationStore:
    def bind(
        self,
        *,
        session_id: WorkspaceSessionId,
        files: WorkspaceFilesProvider,
    ) -> WorkspaceObservationService:
        return ProviderWorkspaceObservationService(files, session_id=session_id)
