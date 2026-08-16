type NativeWorkspaceObservationReceipt = tuple[str, str, str, int, list[tuple[int, int]], bool]
type NativeWorkspaceRenderedLine = tuple[int, str, str]
type NativeWorkspacePolicy = tuple[bool, float, int, int, int, int, bool, int]
type NativeWorkspaceFileRead = tuple[
    str,
    NativeWorkspaceObservationReceipt | None,
    list[NativeWorkspaceRenderedLine],
    int,
    bool,
    bool,
    int,
    int,
]
type NativeWorkspaceDirectoryRead = tuple[str, list[tuple[str, str, int | None]], bool]
type NativeWorkspaceFileChange = tuple[
    str,
    str,
    str | None,
    str | None,
    str | None,
    NativeWorkspaceObservationReceipt | None,
    int,
    int,
]
type NativeWorkspacePostEditSource = tuple[
    str,
    NativeWorkspaceObservationReceipt,
    list[NativeWorkspaceRenderedLine],
    bool,
]
type NativeWorkspaceEditResult = tuple[
    str,
    int,
    int,
    list[NativeWorkspaceFileChange],
    list[NativeWorkspacePostEditSource],
    bool,
    bool,
    str | None,
    float | None,
]

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
class NativeWorkspaceReadError(Exception): ...
class NativeWorkspaceEncodingError(NativeWorkspaceReadError): ...
class NativeWorkspaceBinaryFileError(NativeWorkspaceReadError): ...
class NativeWorkspaceLimitError(Exception): ...
class NativeWorkspaceObservationNotFoundError(Exception): ...
class NativeWorkspaceObservationCollisionError(Exception): ...
class NativeWorkspaceUnseenLineError(Exception): ...
class NativeWorkspaceObservedLineChangedError(Exception): ...
class NativeWorkspaceStaleError(Exception): ...
class NativeWorkspaceEditModeError(Exception): ...
class NativeWorkspacePatchError(Exception): ...
class NativeWorkspacePartialCommitError(Exception): ...
class NativeWorkspaceWriteError(Exception): ...
class NativeWorkspacePathError(Exception): ...
class NativeWorkspaceClosedError(Exception): ...



class NativeAstCancellation:
    def __new__(cls) -> NativeAstCancellation: ...
    def cancel(self) -> None: ...
class NativeWorkspace:
    @property
    def root(self) -> str: ...

class NativeWorkspaceMutation:
    @property
    def mode(self) -> str: ...
    @property
    def mode_generation(self) -> int: ...
    @property
    def policy_generation(self) -> int: ...


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
def workspace_create(root: str) -> NativeWorkspace: ...
def workspace_close(workspace: NativeWorkspace) -> None: ...
def workspace_is_closed(workspace: NativeWorkspace) -> bool: ...
def workspace_revision(workspace: NativeWorkspace) -> int: ...
def workspace_policy(workspace: NativeWorkspace) -> NativeWorkspacePolicy: ...
def workspace_set_policy(
    workspace: NativeWorkspace,
    policy: tuple[bool, float, int, int, int, int, bool],
) -> NativeWorkspacePolicy: ...
def workspace_edit_mode(workspace: NativeWorkspace) -> tuple[str, int]: ...
def workspace_set_edit_mode(workspace: NativeWorkspace, mode: str) -> tuple[str, int]: ...
def workspace_capture_mutation(workspace: NativeWorkspace) -> NativeWorkspaceMutation: ...
def workspace_read_file(
    workspace: NativeWorkspace,
    path: str,
    ranges: list[tuple[int, int | None]],
) -> NativeWorkspaceFileRead: ...
def workspace_list_directory(
    workspace: NativeWorkspace,
    path: str,
    depth: int,
) -> NativeWorkspaceDirectoryRead: ...
def workspace_resolve_observation(
    workspace: NativeWorkspace,
    path: str,
    tag: str,
) -> NativeWorkspaceObservationReceipt: ...
def workspace_validate_observed_lines(
    workspace: NativeWorkspace,
    path: str,
    tag: str,
    lines: list[int],
) -> NativeWorkspaceObservationReceipt: ...
def workspace_create_file(
    workspace: NativeWorkspace,
    path: str,
    content: str,
    create_parents: bool,
) -> NativeWorkspaceEditResult: ...
def workspace_replace_file(
    workspace: NativeWorkspace,
    path: str,
    content: str,
    expected_observation: str,
) -> NativeWorkspaceEditResult: ...
def workspace_replace_text(
    workspace: NativeWorkspace,
    mutation: NativeWorkspaceMutation,
    path: str,
    old_string: str,
    new_string: str,
    replace_all: bool,
) -> NativeWorkspaceEditResult: ...
def workspace_patch(
    workspace: NativeWorkspace,
    mutation: NativeWorkspaceMutation,
    path: str,
    edits: list[tuple[str, str | None, str | None]],
) -> NativeWorkspaceEditResult: ...
def workspace_apply_patch(
    workspace: NativeWorkspace,
    mutation: NativeWorkspaceMutation,
    input: str,
) -> NativeWorkspaceEditResult: ...
def workspace_delete_file(
    workspace: NativeWorkspace,
    path: str,
) -> NativeWorkspaceEditResult: ...
def workspace_move_file(
    workspace: NativeWorkspace,
    path: str,
    destination: str,
) -> NativeWorkspaceEditResult: ...
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
