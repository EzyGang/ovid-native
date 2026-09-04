from ovid_core.models import BaseModel
from pydantic import Field


class WorkspaceCommandRequest(BaseModel):
    command: str = Field(min_length=1, max_length=131_072)
    cwd: str | None = Field(default=None, min_length=1, max_length=4_096)
    env: dict[str, str] = Field(default_factory=dict)
    timeout_seconds: int = Field(default=300, ge=1, le=3_600)
    max_output_bytes: int = Field(default=65_536, ge=1, le=16_777_216)


class WorkspaceCommandResult(BaseModel):
    output: str
    exit_code: int | None
    timed_out: bool
    truncated: bool
    wall_time_ms: int = Field(ge=0)
