from typing import Literal, NotRequired, TypedDict

from pydantic import JsonValue


type WorkspaceReadTruncationReason = Literal[
    'byte_limit',
    'line_limit',
    'per_directory_limit',
    'directory_limit',
]


class WorkspaceReadTruncationMetadata(TypedDict):
    reasons: list[WorkspaceReadTruncationReason]
    continuations: list[str]


class WorkspaceFileReadToolMetadata(TypedDict):
    status: Literal['ok']
    kind: Literal['file']
    requested: str
    path: str
    total_bytes: int
    total_lines: int
    returned_lines: int
    visible_ranges: list[list[int | None]]
    editable: bool
    complete_presentation: bool
    truncated: bool
    observation: NotRequired[str]
    truncation: NotRequired[WorkspaceReadTruncationMetadata]
    unmatched_ranges: NotRequired[list[list[int | None]]]


class WorkspaceDirectoryReadToolMetadata(TypedDict):
    status: Literal['ok']
    kind: Literal['directory']
    requested: str
    path: str
    scanned_entries: int
    tree_lines: int
    returned_lines: int
    per_directory_child_limit: int
    limited_directories: int
    omitted_entries: int
    scan_truncated: bool
    truncated: bool
    truncation: NotRequired[WorkspaceReadTruncationMetadata]


class WorkspaceReadErrorToolMetadata(TypedDict):
    status: Literal['error']
    requested: str
    error: str
    error_type: str


type WorkspaceReadTargetToolMetadata = (
    WorkspaceFileReadToolMetadata | WorkspaceDirectoryReadToolMetadata | WorkspaceReadErrorToolMetadata
)


class WorkspaceMultiReadToolMetadata(TypedDict):
    kind: Literal['multi']
    target_count: int
    successful_targets: int
    failed_targets: int
    truncated: bool
    targets: list[WorkspaceReadTargetToolMetadata]


class WorkspaceFileChangeToolMetadata(TypedDict):
    path: str
    operation: Literal['create', 'update', 'delete', 'move']
    destination: str | None
    before_sha256: str | None
    after_sha256: str | None
    observation: JsonValue
    file_generation: int
    revision: int


class WorkspaceSourcePresentationToolMetadata(TypedDict):
    mode: str
    mode_generation: int
    format: Literal['plain', 'hashline']


class WorkspaceEditToolMetadata(TypedDict):
    mode: str
    mode_generation: int
    policy_generation: int
    changes: list[WorkspaceFileChangeToolMetadata]
    preflight_complete: bool
    commit_complete: bool
    matching_strategy: Literal['exact', 'fuzzy'] | None
    confidence: float | None
    source_presentation: WorkspaceSourcePresentationToolMetadata


type WorkspaceFilesToolMetadata = (
    WorkspaceFileReadToolMetadata
    | WorkspaceDirectoryReadToolMetadata
    | WorkspaceReadErrorToolMetadata
    | WorkspaceMultiReadToolMetadata
    | WorkspaceEditToolMetadata
)
