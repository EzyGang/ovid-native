from datetime import datetime
from typing import Literal

from ovid_core.models import BaseModel
from ovid_core.tools.models import ToolResult
from pydantic import Field


SearchCompletion = Literal['complete', 'file_limit_reached', 'deadline_reached']
GlobOrder = Literal['path', 'modified_desc']
GlobFileType = Literal['file', 'directory', 'any']
GrepPatternMode = Literal['regex', 'literal', 'auto']
GrepRegexEngine = Literal['rust', 'pcre2']
GrepLargeFileMode = Literal['skip', 'prefix']


class SearchScanOptions(BaseModel):
    paths: tuple[str, ...] = ('.',)
    include_hidden: bool = False
    respect_gitignore: bool = True
    include_node_modules: bool = False


class SearchLimits(BaseModel):
    max_scan_files: int = Field(default=10_000, ge=1)
    max_glob_results: int = Field(default=1_000, ge=1)
    max_grep_files: int = Field(default=1_000, ge=1)
    max_grep_matches: int = Field(default=5_000, ge=1)
    max_matches_per_file: int = Field(default=200, ge=1)
    max_file_bytes: int = Field(default=4 * 1024 * 1024, ge=1)
    max_context_lines: int = Field(default=10, ge=0)
    max_line_characters: int = Field(default=2_000, ge=1)
    max_timeout_seconds: float = Field(default=30.0, gt=0)


class GlobRequest(BaseModel):
    patterns: tuple[str, ...] = ('.',)
    include_hidden: bool = False
    respect_gitignore: bool = True
    include_node_modules: bool = False
    file_type: GlobFileType = 'any'
    order: GlobOrder = 'modified_desc'
    limit: int = Field(default=200, ge=1)
    timeout_seconds: float = Field(default=5.0, gt=0)


class GlobMatch(BaseModel):
    path: str
    file_type: GlobFileType
    size: int | None = None
    modified_at: datetime | None = None


class GlobResult(BaseModel):
    matches: tuple[GlobMatch, ...]
    completion: SearchCompletion
    scanned_entries: int
    skipped_entries: int
    truncated: bool


class GrepRequest(BaseModel):
    pattern: str
    scan: SearchScanOptions = Field(default_factory=SearchScanOptions)
    mode: GrepPatternMode = 'regex'
    case_sensitive: bool = True
    multiline: bool = False
    file_offset: int = Field(default=0, ge=0)
    file_limit: int = Field(default=20, ge=1)
    matches_per_file: int = Field(default=20, ge=1)
    context_before: int = Field(default=0, ge=0)
    context_after: int = Field(default=0, ge=0)
    max_file_bytes: int = Field(default=4 * 1024 * 1024, ge=1)
    large_file_mode: GrepLargeFileMode = 'prefix'
    timeout_seconds: float = Field(default=30.0, gt=0)


class GrepToolRequest(GrepRequest):
    mode: GrepPatternMode = 'auto'


class GrepPosition(BaseModel):
    line: int = Field(ge=1)
    column: int = Field(ge=1)
    byte_offset: int = Field(ge=0)


class GrepRange(BaseModel):
    start: GrepPosition
    end: GrepPosition


class GrepContextLine(BaseModel):
    line_number: int = Field(ge=1)
    text: str
    truncated: bool = False


class GrepMatch(BaseModel):
    text: str
    range: GrepRange
    line_text: str
    line_truncated: bool = False
    matched_lines: tuple[GrepContextLine, ...]
    context_before: tuple[GrepContextLine, ...] = ()
    context_after: tuple[GrepContextLine, ...] = ()


class GrepFileCoverage(BaseModel):
    searched_bytes: int
    total_bytes: int
    complete: bool


class GrepFileMatches(BaseModel):
    path: str
    matches: tuple[GrepMatch, ...]
    total_matches: int
    matches_truncated: bool
    total_matches_exact: bool
    coverage: GrepFileCoverage


class GrepResult(BaseModel):
    files: tuple[GrepFileMatches, ...]
    pattern_engine: GrepRegexEngine
    interpreted_as_literal: bool
    completion: SearchCompletion
    files_searched: int
    files_with_matches: int
    files_with_matches_exact: bool
    skipped_binary_files: int
    skipped_encoding_files: int
    skipped_large_files: int
    next_file_offset: int | None
    truncated: bool


class GlobToolContent(BaseModel):
    result: GlobResult


class GrepToolContent(BaseModel):
    result: GrepResult


class GlobToolResult(ToolResult):
    pass


class GrepToolResult(ToolResult):
    pass
