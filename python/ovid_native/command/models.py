from typing import Self

from ovid_core.models import BaseModel
from pydantic import Field, model_validator


class WorkspaceCommandRequest(BaseModel):
    command: str = Field(min_length=1, max_length=131_072)
    cwd: str | None = Field(default=None, min_length=1, max_length=4_096)
    env: dict[str, str] = Field(default_factory=dict)
    timeout_seconds: int = Field(default=300, ge=1, le=3_600)
    max_output_bytes: int = Field(default=65_536, ge=1, le=16_777_216)
    max_head_bytes: int = Field(default=0, ge=0, le=16_777_216)

    @model_validator(mode='after')
    def validate_head_limit(self) -> Self:
        if self.max_head_bytes > self.max_output_bytes:
            raise ValueError('max_head_bytes cannot exceed max_output_bytes')
        return self


class WorkspaceCommandResult(BaseModel):
    head: str
    tail: str
    omitted_bytes: int = Field(ge=0)
    exit_code: int | None
    timed_out: bool
    wall_time_ms: int = Field(ge=0)
