from datetime import datetime
from typing import Literal

from ovid_core.models import BaseModel
from ovid_core.tools.models import ToolResult
from pydantic import Field


FffIndexState = Literal['new', 'indexing', 'ready', 'failed', 'closed']
FffGitStatus = Literal['clean', 'modified', 'staged', 'untracked']
FffGitStatusValue = Literal[
    'clean', 'modified', 'staged', 'deleted', 'renamed', 'untracked', 'ignored', 'conflicted', 'unknown'
]
FffFindKind = Literal['file', 'directory', 'any']
FffPathKind = Literal['file', 'directory']
FffGrepMode = Literal['plain', 'regex', 'fuzzy', 'auto']
FffActualGrepMode = Literal['plain', 'regex', 'fuzzy']
FffSearchCompletion = Literal['complete', 'page_limit_reached', 'time_budget_reached', 'index_incomplete']


class FffConfig(BaseModel):
    watch: bool = True
    enable_content_indexing: bool = True
    enable_mmap_cache: bool = False
    initial_scan_timeout_seconds: float = Field(default=30.0, gt=0)
    search_timeout_seconds: float = Field(default=5.0, gt=0)


class FffLimits(BaseModel):
    max_results: int = Field(default=200, ge=1)
    max_matches_per_file: int = Field(default=100, ge=1)
    max_patterns: int = Field(default=32, ge=1)
    max_pattern_characters: int = Field(default=1_000, ge=1)
    max_query_characters: int = Field(default=2_000, ge=1)
    max_file_bytes: int = Field(default=10 * 1024 * 1024, ge=1)
    max_context_lines: int = Field(default=10, ge=0)
    max_search_timeout_seconds: float = Field(default=30.0, gt=0)


class FffIndexStatus(BaseModel):
    state: FffIndexState
    indexed_files: int
    scan_complete: bool
    watch_enabled: bool
    content_index_enabled: bool


class FffConstraints(BaseModel):
    include: tuple[str, ...] = ()
    exclude: tuple[str, ...] = ()
    git_status: FffGitStatus | None = None


class FffFindRequest(BaseModel):
    query: str
    constraints: FffConstraints = Field(default_factory=FffConstraints)
    kind: FffFindKind = 'file'
    offset: int = Field(default=0, ge=0)
    limit: int = Field(default=20, ge=1)


class FffPathMatch(BaseModel):
    path: str
    kind: FffPathKind
    exact_match: bool
    size: int | None = None
    modified_at: datetime | None = None
    git_status: FffGitStatusValue = 'unknown'


class FffFindResult(BaseModel):
    matches: tuple[FffPathMatch, ...]
    total_matches: int
    next_offset: int | None
    index_complete: bool


class FffGrepRequest(BaseModel):
    query: str
    constraints: FffConstraints = Field(default_factory=FffConstraints)
    mode: FffGrepMode = 'auto'
    smart_case: bool = True
    file_offset: int = Field(default=0, ge=0)
    limit: int = Field(default=20, ge=1)
    matches_per_file: int = Field(default=10, ge=1)
    context_before: int = Field(default=0, ge=0)
    context_after: int = Field(default=0, ge=0)
    max_file_bytes: int = Field(default=10 * 1024 * 1024, ge=1)
    timeout_seconds: float = Field(default=5.0, gt=0)
    classify_definitions: bool = True


class FffMultiGrepRequest(BaseModel):
    patterns: tuple[str, ...] = Field(min_length=1)
    constraints: FffConstraints = Field(default_factory=FffConstraints)
    smart_case: bool = True
    file_offset: int = Field(default=0, ge=0)
    limit: int = Field(default=20, ge=1)
    matches_per_file: int = Field(default=10, ge=1)
    context_before: int = Field(default=0, ge=0)
    context_after: int = Field(default=0, ge=0)
    max_file_bytes: int = Field(default=10 * 1024 * 1024, ge=1)
    timeout_seconds: float = Field(default=5.0, gt=0)
    classify_definitions: bool = True


class FffByteRange(BaseModel):
    start: int = Field(ge=0)
    end: int = Field(ge=0)


class FffContextLine(BaseModel):
    line_number: int = Field(ge=1)
    text: str


class FffGrepMatch(BaseModel):
    path: str
    line_number: int = Field(ge=1)
    column: int = Field(ge=1)
    byte_offset: int = Field(ge=0)
    line: str
    match_ranges: tuple[FffByteRange, ...]
    context_before: tuple[FffContextLine, ...] = ()
    context_after: tuple[FffContextLine, ...] = ()
    approximate: bool = False
    is_definition: bool = False
    git_status: FffGitStatusValue = 'unknown'


class FffGrepResult(BaseModel):
    matches: tuple[FffGrepMatch, ...]
    actual_mode: FffActualGrepMode
    fallback_from: Literal['plain', 'regex'] | None = None
    approximate: bool
    completion: FffSearchCompletion
    indexed_files: int
    searchable_files: int
    files_searched: int
    files_with_matches: int
    next_file_offset: int | None
    index_complete: bool
    workspace_revision: str | None = None


class FffFindToolContent(BaseModel):
    result: FffFindResult


class FffGrepToolContent(BaseModel):
    result: FffGrepResult


class FffMultiGrepToolContent(BaseModel):
    result: FffGrepResult


class FffFindToolResult(ToolResult):
    pass


class FffGrepToolResult(ToolResult):
    pass


class FffMultiGrepToolResult(ToolResult):
    pass
