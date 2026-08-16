from typing import Annotated, Literal, Self

from ovid_core.models import BaseModel
from ovid_core.tools.models import ToolResult
from pydantic import AfterValidator, Field, model_validator

from ovid_native.workspace.observations import WorkspaceObservationReceipt, WorkspaceRenderedLine


class ReadLineRange(BaseModel):
    start: int = Field(ge=1)
    end: int | None = Field(default=None, ge=1)

    @model_validator(mode='after')
    def validate_order(self) -> Self:
        if self.end is not None and self.end < self.start:
            raise ValueError('read line range end cannot precede start')
        return self


def _validate_read_ranges(ranges: tuple[ReadLineRange, ...]) -> tuple[ReadLineRange, ...]:
    ordered = sorted(ranges, key=lambda line_range: line_range.start)
    for previous, current in zip(ordered, ordered[1:], strict=False):
        if previous.end is None or current.start <= previous.end:
            raise ValueError('read line ranges cannot overlap')
    return ranges


type ReadLineRanges = Annotated[tuple[ReadLineRange, ...], AfterValidator(_validate_read_ranges)]


class WorkspaceFileReadRequest(BaseModel):
    path: str = Field(min_length=1)
    ranges: ReadLineRanges = ()


class WorkspaceDirectoryReadRequest(BaseModel):
    path: str = Field(min_length=1)
    depth: int = Field(default=1, ge=1, le=2)


class WorkspaceReadRequest(BaseModel):
    path: str = Field(min_length=1)
    ranges: ReadLineRanges = ()
    directory_depth: int = Field(default=1, ge=1, le=2)


class WorkspaceReadFileResult(BaseModel):
    kind: Literal['file'] = 'file'
    path: str
    observation: WorkspaceObservationReceipt | None
    lines: tuple[WorkspaceRenderedLine, ...]
    total_lines: int = Field(ge=0)
    complete_presentation: bool
    editable: bool
    total_bytes: int = Field(ge=0)
    observation_limit: int = Field(ge=1)

    def render(self) -> str:
        header_tag = self.observation.tag if self.observation is not None else '----'
        rows = [f'[{self.path}#{header_tag}]']
        rows.extend(f'{line.line_number}:{line.short_hash}|{line.text}' for line in self.lines)
        if not self.complete_presentation:
            rows.append(
                f'[truncated: {len(self.lines)} of {self.total_lines} lines; '
                f'{self.total_bytes} bytes, observation limit {self.observation_limit}]'
            )
        return '\n'.join(rows)


class WorkspaceDirectoryEntry(BaseModel):
    path: str
    kind: Literal['file', 'directory', 'symlink']
    size: int | None = Field(default=None, ge=0)


class WorkspaceReadDirectoryResult(BaseModel):
    kind: Literal['directory'] = 'directory'
    path: str
    entries: tuple[WorkspaceDirectoryEntry, ...]
    truncated: bool

    def render(self) -> str:
        rows = [f'[{self.path}]']
        rows.extend(f'{entry.path}{"/" if entry.kind == "directory" else ""}' for entry in self.entries)
        if self.truncated:
            rows.append('[directory listing truncated]')
        return '\n'.join(rows)


type WorkspaceReadResult = WorkspaceReadFileResult | WorkspaceReadDirectoryResult


class WorkspaceCreateRequest(BaseModel):
    path: str = Field(min_length=1)
    content: str
    create_parents: bool = False


class WorkspaceReplaceRequest(BaseModel):
    path: str = Field(min_length=1)
    content: str
    expected_observation: str = Field(pattern=r'^[0-9A-Fa-f]{4}$')


class WorkspaceDeleteRequest(BaseModel):
    path: str = Field(min_length=1)


class WorkspaceMoveRequest(BaseModel):
    path: str = Field(min_length=1)
    destination: str = Field(min_length=1)


class WorkspaceWriteRequest(BaseModel):
    path: str = Field(min_length=1)
    content: str
    operation: Literal['create', 'replace'] = 'create'
    expected_observation: str | None = Field(default=None, pattern=r'^[0-9A-Fa-f]{4}$')
    create_parents: bool = False

    @model_validator(mode='after')
    def validate_observation(self) -> Self:
        if self.operation == 'replace' and self.expected_observation is None:
            raise ValueError('replace requires expected_observation')
        if self.operation == 'create' and self.expected_observation is not None:
            raise ValueError('create does not accept expected_observation')
        if self.operation == 'replace' and self.create_parents:
            raise ValueError('replace does not accept create_parents')
        return self


class WorkspaceFileChange(BaseModel):
    path: str
    operation: Literal['create', 'update', 'delete', 'move']
    destination: str | None = None
    before_sha256: str | None = None
    after_sha256: str | None = None
    observation: WorkspaceObservationReceipt | None = None
    file_generation: int = Field(ge=1)
    revision: int = Field(ge=1)


class WorkspacePostEditSource(BaseModel):
    path: str
    observation: WorkspaceObservationReceipt
    lines: tuple[WorkspaceRenderedLine, ...]
    complete_presentation: bool

    def render(self) -> str:
        rows = [f'[{self.path}#{self.observation.tag}]']
        rows.extend(f'{line.line_number}:{line.short_hash}|{line.text}' for line in self.lines)
        return '\n'.join(rows)


class WorkspaceEditResult(BaseModel):
    mode: str
    mode_generation: int = Field(ge=1)
    policy_generation: int = Field(ge=1)
    changes: tuple[WorkspaceFileChange, ...]
    post_edit_sources: tuple[WorkspacePostEditSource, ...]
    preflight_complete: bool
    commit_complete: bool
    matching_strategy: Literal['exact', 'fuzzy'] | None = None
    confidence: float | None = Field(default=None, ge=0, le=1)


class WorkspaceWriteResult(WorkspaceEditResult):
    mode: Literal['write'] = 'write'


class ReplaceEditRequest(BaseModel):
    path: str = Field(min_length=1)
    old_string: str = Field(min_length=1)
    new_string: str
    replace_all: bool = False


class PatchEditEntry(BaseModel):
    operation: Literal['create', 'update', 'delete']
    diff: str | None = None
    destination: str | None = None

    @model_validator(mode='after')
    def validate_entry(self) -> Self:
        if self.operation in ('create', 'update') and self.diff is None:
            raise ValueError(f'{self.operation} patch entry requires diff')
        if self.operation != 'update' and self.destination is not None:
            raise ValueError(f'{self.operation} patch entry does not accept destination')
        if self.operation == 'delete' and self.diff is not None:
            raise ValueError('delete patch entry does not accept diff')
        return self


class PatchEditRequest(BaseModel):
    path: str = Field(min_length=1)
    edits: tuple[PatchEditEntry, ...] = Field(min_length=1)


class ApplyPatchEditRequest(BaseModel):
    input: str = Field(min_length=1)


class WorkspaceFilesToolResult(ToolResult):
    pass
