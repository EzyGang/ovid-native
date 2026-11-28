from typing import Literal, Self

from ovid_core.models import BaseModel
from pydantic import Field, model_validator


class WorkspaceCommandRequest(BaseModel):
    command: str = Field(min_length=1, max_length=131_072)
    cwd: str | None = Field(default=None, min_length=1, max_length=4_096)
    env: dict[str, str] = Field(default_factory=dict)
    timeout_seconds: int = Field(default=300, ge=1, le=3_600)
    max_output_bytes: int = Field(default=65_536, ge=1, le=16_777_216)
    max_head_bytes: int = Field(default=0, ge=0, le=16_777_216)
    owner_tag: str | None = Field(default=None, min_length=1, max_length=128)

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


type CommandJobStatus = Literal['running', 'exited', 'failed', 'timed_out', 'killed']
type CommandJobSignal = Literal['interrupt', 'terminate', 'kill']


class WorkspaceCommandJobRequest(BaseModel):
    command: str = Field(min_length=1, max_length=131_072)
    cwd: str | None = Field(default=None, min_length=1, max_length=4_096)
    env: dict[str, str] = Field(default_factory=dict)
    timeout_seconds: int | None = Field(default=None, ge=1, le=86_400)
    pty: bool = False
    cols: int = Field(default=120, ge=1, le=1_000)
    rows: int = Field(default=40, ge=1, le=1_000)
    max_buffer_bytes: int = Field(default=1_048_576, ge=65_536, le=8_388_608)
    owner_tag: str | None = Field(default=None, min_length=1, max_length=128)


class WorkspaceCommandJobSnapshot(BaseModel):
    job_id: int = Field(ge=1)
    status: CommandJobStatus
    exit_code: int | None
    head: str
    tail: str
    omitted_bytes: int = Field(ge=0)
    next_offset: int = Field(ge=0)
    truncated_before: int = Field(ge=0)
    wall_time_ms: int = Field(ge=0)


class ReapedProcess(BaseModel):
    pid: int = Field(ge=1)
    owner_tag: str | None
    command_hash_prefix: str


class WorkspaceReapReport(BaseModel):
    stopped: tuple[ReapedProcess, ...] = ()
    skipped_live_owners: int = Field(default=0, ge=0)
