from datetime import datetime
from typing import Literal

from ovid_core.models import BaseModel
from ovid_core.tools.models import ToolResult
from pydantic import Field

from ovid_native.workspace.evidence import WorkspaceSourceLineClaim


AstStrictness = Literal['cst', 'smart', 'ast', 'relaxed', 'signature', 'template']
AstIssueKind = Literal['unsupported_language', 'invalid_pattern', 'parse_error', 'read_error', 'limit_reached']


class AstLanguageInfo(BaseModel):
    identifier: str
    aliases: tuple[str, ...]
    extensions: tuple[str, ...]


class AstPosition(BaseModel):
    line: int = Field(ge=1, description='One-based source line.')
    column: int = Field(ge=1, description='One-based Unicode source column.')
    byte_offset: int = Field(ge=0, description='Zero-based UTF-8 byte offset.')


class AstRange(BaseModel):
    start: AstPosition = Field(description='Inclusive one-based line and column with a zero-based byte offset.')
    end: AstPosition = Field(description='Exclusive one-based line and column with a zero-based byte offset.')


class AstCapture(BaseModel):
    name: str
    text: str
    range: AstRange | None = None


class AstIssue(BaseModel):
    path: str | None = None
    language: str | None = None
    kind: AstIssueKind
    message: str


class AstScanOptions(BaseModel):
    paths: tuple[str, ...] = ('.',)
    include_hidden: bool = False
    respect_gitignore: bool = True
    include_node_modules: bool = False


class AstLimits(BaseModel):
    max_matches: int = Field(default=500, ge=1)
    max_files: int = Field(default=10_000, ge=1)
    max_file_bytes: int = Field(default=4 * 1024 * 1024, ge=1)
    max_replacements: int = Field(default=5_000, ge=1)
    max_changed_files: int = Field(default=1_000, ge=1)
    proposal_ttl_seconds: int = Field(default=600, ge=1)
    max_pending_proposals: int = Field(default=32, ge=1)


class AstSearchRequest(BaseModel):
    pattern: str
    scan: AstScanOptions = AstScanOptions()
    language: str | None = None
    strictness: AstStrictness = 'smart'
    offset: int = Field(default=0, ge=0)
    limit: int = Field(default=50, ge=1)
    include_captures: bool = True


class AstMatch(BaseModel):
    path: str
    language: str
    text: str
    range: AstRange
    captures: tuple[AstCapture, ...] = ()
    source_lines: tuple[WorkspaceSourceLineClaim, ...] = ()


class AstSearchResult(BaseModel):
    matches: tuple[AstMatch, ...]
    total_matches: int
    files_searched: int
    files_with_matches: int
    unsupported_files: int
    truncated: bool
    issues: tuple[AstIssue, ...] = ()


class AstRewriteOperation(BaseModel):
    pattern: str
    replacement: str


class AstRewritePreviewRequest(BaseModel):
    operations: tuple[AstRewriteOperation, ...] = Field(min_length=1)
    scan: AstScanOptions
    language: str | None = None
    strictness: AstStrictness = 'smart'


class AstChange(BaseModel):
    path: str
    language: str
    before: str
    after: str
    range: AstRange


class AstFileChange(BaseModel):
    path: str
    original_sha256: str
    updated_sha256: str
    replacements: int


class AstRewritePreview(BaseModel):
    proposal_id: str
    changes: tuple[AstChange, ...]
    files: tuple[AstFileChange, ...]
    total_replacements: int
    files_searched: int
    expires_at: datetime
    issues: tuple[AstIssue, ...] = ()


class AstRewriteApplyRequest(BaseModel):
    proposal_id: str


class AstRewriteApplyResult(BaseModel):
    proposal_id: str
    files: tuple[AstFileChange, ...]
    total_replacements: int


class AstSearchToolContent(BaseModel):
    result: AstSearchResult


class AstRewritePreviewToolContent(BaseModel):
    preview: AstRewritePreview


class AstRewriteApplyToolContent(BaseModel):
    result: AstRewriteApplyResult


class AstSearchToolResult(ToolResult):
    pass


class AstRewritePreviewToolResult(ToolResult):
    pass


class AstRewriteApplyToolResult(ToolResult):
    pass
