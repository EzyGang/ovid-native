type NativeAstPosition = tuple[int, int, int]
type NativeAstRange = tuple[NativeAstPosition, NativeAstPosition]
type NativeAstCapture = tuple[str, str, NativeAstRange | None]
type NativeAstMatch = tuple[str, str, str, NativeAstRange, list[NativeAstCapture]]
type NativeAstIssue = tuple[str | None, str | None, str, str]
type NativeAstSearchResult = tuple[list[NativeAstMatch], int, int, int, int, bool, list[NativeAstIssue]]
type NativeAstChange = tuple[str, str, str, str, NativeAstRange]
type NativeAstFileChange = tuple[str, str, str, int]
type NativeAstLanguageInfo = tuple[str, list[str], list[str]]


class NativeAstConfigurationError(Exception): ...
class NativeAstPathError(Exception): ...
class NativeAstLanguageError(Exception): ...
class NativeAstPatternError(Exception): ...
class NativeAstLimitError(Exception): ...
class NativeAstProposalStaleError(Exception): ...
class NativeAstWriteError(Exception): ...
class NativeAstCancelledError(Exception): ...


class NativeAstCancellation:
    def __new__(cls) -> NativeAstCancellation: ...
    def cancel(self) -> None: ...


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
