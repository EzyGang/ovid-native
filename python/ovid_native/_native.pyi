type NativeAstPosition = tuple[int, int, int]
type NativeAstRange = tuple[NativeAstPosition, NativeAstPosition]
type NativeAstCapture = tuple[str, str, NativeAstRange | None]
type NativeAstMatch = tuple[str, str, str, NativeAstRange, list[NativeAstCapture]]
type NativeAstIssue = tuple[str | None, str | None, str, str]
type NativeAstSearchResult = tuple[list[NativeAstMatch], int, int, int, int, bool, list[NativeAstIssue]]
type NativeAstChange = tuple[str, str, str, str, NativeAstRange]
type NativeAstFileChange = tuple[str, str, str, int]
type NativeAstLanguageInfo = tuple[str, list[str], list[str]]
type NativeGlobMatch = tuple[str, str, int | None, float | None]
type NativeGlobResult = tuple[list[NativeGlobMatch], str, int, int, bool]
type NativeGrepPosition = tuple[int, int, int]
type NativeGrepRange = tuple[NativeGrepPosition, NativeGrepPosition]
type NativeGrepContextLine = tuple[int, str, bool]
type NativeGrepMatch = tuple[
    str,
    NativeGrepRange,
    str,
    bool,
    list[NativeGrepContextLine],
    list[NativeGrepContextLine],
]
type NativeGrepCoverage = tuple[int, int, bool]
type NativeGrepFileMatches = tuple[str, list[NativeGrepMatch], int, bool, bool, NativeGrepCoverage]
type NativeGrepResult = tuple[
    list[NativeGrepFileMatches],
    str,
    bool,
    str,
    int,
    int,
    bool,
    int,
    int,
    int,
    int | None,
    bool,
]
type NativeFffIndexStatus = tuple[str, int, bool, bool, bool]
type NativeFffPathMatch = tuple[str, str, bool, int | None, int | None, str]
type NativeFffFindResult = tuple[list[NativeFffPathMatch], int, int | None, bool]
type NativeFffByteRange = tuple[int, int]
type NativeFffContextLine = tuple[int, str]
type NativeFffGrepMatch = tuple[
    str,
    int,
    int,
    int,
    str,
    list[NativeFffByteRange],
    list[NativeFffContextLine],
    list[NativeFffContextLine],
    bool,
    bool,
    str,
]
type NativeFffGrepResult = tuple[
    list[NativeFffGrepMatch],
    str,
    str | None,
    bool,
    str,
    int,
    int,
    int,
    int,
    int | None,
    bool,
]


class NativeWorkspaceConfigurationError(Exception): ...
class NativeWorkspacePathError(Exception): ...
class NativeWorkspaceClosedError(Exception): ...
class NativeAstConfigurationError(Exception): ...
class NativeAstPathError(Exception): ...
class NativeAstLanguageError(Exception): ...
class NativeAstPatternError(Exception): ...
class NativeAstLimitError(Exception): ...
class NativeAstProposalStaleError(Exception): ...
class NativeAstWriteError(Exception): ...
class NativeAstCancelledError(Exception): ...
class NativeSearchConfigurationError(Exception): ...
class NativeSearchPathError(Exception): ...
class NativeSearchPatternError(Exception): ...
class NativeSearchLimitError(Exception): ...
class NativeSearchCancelledError(Exception): ...
class NativeSearchReadError(Exception): ...
class NativeFffConfigurationError(Exception): ...
class NativeFffPathError(Exception): ...
class NativeFffQueryError(Exception): ...
class NativeFffPatternError(Exception): ...
class NativeFffLimitError(Exception): ...
class NativeFffIndexNotReadyError(Exception): ...
class NativeFffClosedError(Exception): ...
class NativeFffCancelledError(Exception): ...
class NativeFffRuntimeError(Exception): ...
class NativeFffStartupError(Exception): ...


class NativeAstCancellation:
    def __new__(cls) -> NativeAstCancellation: ...
    def cancel(self) -> None: ...
class NativeWorkspace:
    @property
    def root(self) -> str: ...
    @property
    def session_id(self) -> str: ...
    @property
    def revision(self) -> int: ...


class NativeSearchCancellation:
    def __new__(cls) -> NativeSearchCancellation: ...
    def cancel(self) -> None: ...


class NativeFffEngine: ...


class NativeFffCancellation:
    def __new__(cls) -> NativeFffCancellation: ...
    def cancel(self) -> None: ...


class NativeFffConfig:
    def __new__(
        cls,
        watch: bool,
        enable_content_indexing: bool,
        enable_mmap_cache: bool,
        initial_scan_timeout_seconds: float,
        search_timeout_seconds: float,
    ) -> NativeFffConfig: ...


class NativeFffLimits:
    def __new__(
        cls,
        max_results: int,
        max_matches_per_file: int,
        max_patterns: int,
        max_pattern_characters: int,
        max_query_characters: int,
        max_file_bytes: int,
        max_context_lines: int,
        max_search_timeout_seconds: float,
    ) -> NativeFffLimits: ...


class NativeFffFindRequest:
    def __new__(
        cls,
        query: str,
        constraints: tuple[list[str], list[str], str | None],
        kind: str,
        offset: int,
        limit: int,
    ) -> NativeFffFindRequest: ...


class NativeFffGrepRequest:
    def __new__(
        cls,
        query: str,
        constraints: tuple[list[str], list[str], str | None],
        matching: tuple[str, bool],
        pagination: tuple[int, int, int],
        content: tuple[int, int, int, float, bool],
    ) -> NativeFffGrepRequest: ...


class NativeFffMultiGrepRequest:
    def __new__(
        cls,
        patterns: list[str],
        constraints: tuple[list[str], list[str], str | None],
        smart_case: bool,
        pagination: tuple[int, int, int],
        content: tuple[int, int, int, float, bool],
    ) -> NativeFffMultiGrepRequest: ...
class NativeGlobRequest:
    def __new__(
        cls,
        patterns: list[str],
        scan_flags: tuple[bool, bool, bool],
        options: tuple[str, str, int, int, float],
        cancellation: NativeSearchCancellation,
    ) -> NativeGlobRequest: ...


class NativeGrepRequest:
    def __new__(
        cls,
        pattern: str,
        scan: tuple[list[str], bool, bool, bool],
        matching: tuple[str, bool, bool],
        pagination: tuple[int, int, int],
        content: tuple[int, int, int, str, float],
        limits: tuple[int, int, int, int],
        cancellation: NativeSearchCancellation,
    ) -> NativeGrepRequest: ...


class NativeAstLimits:
    def __new__(
        cls,
        max_matches: int,
        max_files: int,
        max_file_bytes: int,
        max_replacements: int,
        max_changed_files: int,
    ) -> NativeAstLimits: ...


class NativeAstScanOptions:
    def __new__(
        cls,
        paths: list[str],
        include_hidden: bool,
        respect_gitignore: bool,
        include_node_modules: bool,
    ) -> NativeAstScanOptions: ...


class NativeAstSearchRequest:
    def __new__(
        cls,
        pattern: str,
        scan: NativeAstScanOptions,
        language: str | None,
        strictness: str,
        options: tuple[int, int, bool],
        limits: NativeAstLimits,
        cancellation: NativeAstCancellation,
    ) -> NativeAstSearchRequest: ...


class NativeAstRewriteRequest:
    def __new__(
        cls,
        operations: list[tuple[str, str]],
        scan: NativeAstScanOptions,
        language: str | None,
        strictness: str,
        limits: NativeAstLimits,
        cancellation: NativeAstCancellation,
    ) -> NativeAstRewriteRequest: ...


class NativeAstRewriteComputation: ...


def runtime_info() -> tuple[str, str, int]: ...
def workspace_create(root: str, session_id: str) -> NativeWorkspace: ...
def workspace_close(workspace: NativeWorkspace) -> None: ...
def search_glob(workspace: NativeWorkspace, request: NativeGlobRequest) -> NativeGlobResult: ...
def search_grep(workspace: NativeWorkspace, request: NativeGrepRequest) -> NativeGrepResult: ...
def fff_create(workspace: NativeWorkspace, config: NativeFffConfig, limits: NativeFffLimits) -> NativeFffEngine: ...
def fff_start(engine: NativeFffEngine) -> NativeFffIndexStatus: ...
def fff_wait_ready(engine: NativeFffEngine, timeout_seconds: float) -> NativeFffIndexStatus: ...
def fff_status(engine: NativeFffEngine) -> NativeFffIndexStatus: ...
def fff_rescan(engine: NativeFffEngine) -> NativeFffIndexStatus: ...
def fff_close(engine: NativeFffEngine) -> None: ...
def fff_find(engine: NativeFffEngine, request: NativeFffFindRequest) -> NativeFffFindResult: ...
def fff_grep(
    engine: NativeFffEngine,
    request: NativeFffGrepRequest,
    cancellation: NativeFffCancellation,
) -> NativeFffGrepResult: ...
def fff_multi_grep(
    engine: NativeFffEngine,
    request: NativeFffMultiGrepRequest,
    cancellation: NativeFffCancellation,
) -> NativeFffGrepResult: ...
def fff_version() -> str: ...
def ast_supported_languages() -> list[NativeAstLanguageInfo]: ...
def ast_grep_version() -> str: ...
def ast_search(workspace: NativeWorkspace, request: NativeAstSearchRequest) -> NativeAstSearchResult: ...
def ast_preview_rewrite(
    workspace: NativeWorkspace,
    request: NativeAstRewriteRequest,
) -> tuple[
    NativeAstRewriteComputation,
    list[NativeAstChange],
    list[NativeAstFileChange],
    int,
    int,
    list[NativeAstIssue],
]: ...
def ast_apply_rewrite(
    workspace: NativeWorkspace,
    computation: NativeAstRewriteComputation,
    cancellation: NativeAstCancellation,
) -> tuple[list[NativeAstFileChange], int]: ...
