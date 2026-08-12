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


class NativeAstCancellation:
    def __new__(cls) -> NativeAstCancellation: ...
    def cancel(self) -> None: ...
class NativeWorkspace:
    @property
    def root(self) -> str: ...


class NativeSearchCancellation:
    def __new__(cls) -> NativeSearchCancellation: ...
    def cancel(self) -> None: ...


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


def runtime_info() -> tuple[str, str, int, str | None]: ...
def search_workspace(root: str) -> NativeWorkspace: ...
def search_glob(workspace: NativeWorkspace, request: NativeGlobRequest) -> NativeGlobResult: ...
def search_grep(workspace: NativeWorkspace, request: NativeGrepRequest) -> NativeGrepResult: ...
def ast_supported_languages() -> list[NativeAstLanguageInfo]: ...
def ast_grep_version() -> str: ...
def ast_search(root: str, request: NativeAstSearchRequest) -> NativeAstSearchResult: ...
def ast_preview_rewrite(
    root: str,
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
    root: str,
    computation: NativeAstRewriteComputation,
    cancellation: NativeAstCancellation,
) -> tuple[list[NativeAstFileChange], int]: ...
