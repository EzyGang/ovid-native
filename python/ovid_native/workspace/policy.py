from ovid_core.models import BaseModel
from pydantic import Field

from ovid_native import _native
from ovid_native.workspace.errors import WorkspaceClosedError


class WorkspacePolicy(BaseModel):
    allow_fuzzy_replace: bool = False
    fuzzy_replace_threshold: float = Field(default=0.9, ge=0, le=1)
    max_read_bytes: int = Field(default=4 * 1024 * 1024, ge=1)
    max_observation_file_bytes: int = Field(default=64 * 1024 * 1024, ge=1)
    max_observation_entries: int = Field(default=4096, ge=1)
    max_observation_store_bytes: int = Field(default=8 * 1024 * 1024, ge=1)
    create_parent_directories: bool = False


class WorkspacePolicyGeneration(BaseModel):
    policy: WorkspacePolicy
    generation: int = Field(ge=1)


class WorkspacePolicyState:
    def __init__(self, workspace: _native.NativeWorkspace) -> None:
        self._workspace = workspace

    @property
    def current(self) -> WorkspacePolicyGeneration:
        self._ensure_open()
        return _policy_generation(_native.workspace_policy(self._workspace))

    def set(self, policy: WorkspacePolicy) -> WorkspacePolicyGeneration:
        self._ensure_open()
        native = _native.workspace_set_policy(
            self._workspace,
            (
                policy.allow_fuzzy_replace,
                policy.fuzzy_replace_threshold,
                policy.max_read_bytes,
                policy.max_observation_file_bytes,
                policy.max_observation_entries,
                policy.max_observation_store_bytes,
                policy.create_parent_directories,
            ),
        )
        return _policy_generation(native)

    def update(self, **changes: bool | float) -> WorkspacePolicyGeneration:
        values = self.current.policy.model_dump()
        values.update(changes)
        return self.set(WorkspacePolicy.model_validate(values))

    def _ensure_open(self) -> None:
        if _native.workspace_is_closed(self._workspace):
            raise WorkspaceClosedError('Workspace session is closed')


def _policy_generation(native: _native.NativeWorkspacePolicy) -> WorkspacePolicyGeneration:
    (
        allow_fuzzy_replace,
        fuzzy_replace_threshold,
        max_read_bytes,
        max_observation_file_bytes,
        max_observation_entries,
        max_observation_store_bytes,
        create_parent_directories,
        generation,
    ) = native
    return WorkspacePolicyGeneration(
        policy=WorkspacePolicy(
            allow_fuzzy_replace=allow_fuzzy_replace,
            fuzzy_replace_threshold=fuzzy_replace_threshold,
            max_read_bytes=max_read_bytes,
            max_observation_file_bytes=max_observation_file_bytes,
            max_observation_entries=max_observation_entries,
            max_observation_store_bytes=max_observation_store_bytes,
            create_parent_directories=create_parent_directories,
        ),
        generation=generation,
    )
