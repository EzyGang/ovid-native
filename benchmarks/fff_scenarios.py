import asyncio
from collections.abc import Awaitable, Callable
from pathlib import Path

from ovid_native.fff import FffConfig, FffEngine, FffFindRequest, FffGrepRequest, FffLimits, FffMultiGrepRequest
from ovid_native.search import GlobRequest, GrepRequest, SearchEngine, SearchLimits, SearchScanOptions


type Measure = Callable[[int], float]
type AsyncMeasure = Callable[[Callable[[], Awaitable[object]]], Measure]
type BenchmarkCase = tuple[str, str, str, Measure, int, str]
type BenchmarkCases = tuple[tuple[BenchmarkCase, ...], Callable[[], None]]


def fff_benchmark_cases(fixture_root: Path, loop: asyncio.AbstractEventLoop, measure: AsyncMeasure) -> BenchmarkCases:
    scales = (
        ('small.100', 100, fixture_root / 'search/src/dir-000', 'file-099.py', 'file 099'),
        ('medium.1k', 1_000, fixture_root / 'search-medium', 'dir-009/file-099.py', 'dir 009 file 099'),
        ('large.10k', 10_000, fixture_root / 'search/src', 'dir-099/file-099.py', 'dir 099 file 099'),
    )
    cases: list[BenchmarkCase] = []
    engines: list[FffEngine] = []

    for scale, file_count, root, exact_path, fuzzy_query in scales:
        native_engine = _native_engine(root)
        fff_engine = _fff_engine(root)
        loop.run_until_complete(fff_engine.wait_ready())
        engines.append(fff_engine)
        cases.extend(_scale_cases(scale, file_count, exact_path, fuzzy_query, native_engine, fff_engine, measure))

    cases.append(_multi_grep_case(engines[-1], measure))

    def close() -> None:
        loop.run_until_complete(_close_all(engines))

    return tuple(cases), close


def _native_engine(root: Path) -> SearchEngine:
    return SearchEngine(
        root=root,
        limits=SearchLimits(
            max_scan_files=20_000,
            max_glob_results=20_000,
            max_grep_files=20_000,
            max_grep_matches=20_000,
        ),
    )


def _fff_engine(root: Path) -> FffEngine:
    return FffEngine(
        root=root,
        config=FffConfig(watch=False),
        limits=FffLimits(max_results=20_010, max_matches_per_file=10),
    )


def _scale_cases(
    scale: str,
    file_count: int,
    exact_path: str,
    fuzzy_query: str,
    native_engine: SearchEngine,
    fff_engine: FffEngine,
    measure: AsyncMeasure,
) -> tuple[BenchmarkCase, ...]:
    glob_request = GlobRequest(patterns=(exact_path,), order='path', limit=20_000)
    native_grep_request = GrepRequest(
        pattern='needle value', mode='literal', scan=SearchScanOptions(paths=('**/*.py',)), file_limit=20_000
    )
    fff_grep_request = FffGrepRequest(query='needle value', mode='plain', limit=20_000)

    return (
        (
            f'glob.exact_file.{scale}',
            'glob',
            'Exact path lookup baseline for fuzzy path discovery',
            measure(lambda: native_engine.glob(glob_request)),
            file_count,
            'files',
        ),
        (
            f'grep.literal_sparse.{scale}',
            'grep',
            'Native literal sparse-search baseline',
            measure(lambda: native_engine.grep(native_grep_request)),
            file_count,
            'files',
        ),
        (
            f'fff.find_typo.{scale}',
            'fff_find',
            'Warm-indexed typo-resistant path ranking',
            measure(lambda: fff_engine.find(FffFindRequest(query=fuzzy_query))),
            file_count,
            'indexed files',
        ),
        (
            f'fff.grep_plain_sparse.{scale}',
            'fff_grep',
            'Warm-indexed sparse plain-text matching',
            measure(lambda: fff_engine.grep(fff_grep_request)),
            file_count,
            'indexed files',
        ),
    )


def _multi_grep_case(engine: FffEngine, measure: AsyncMeasure) -> BenchmarkCase:
    request = FffMultiGrepRequest(patterns=('needle value', 'unicode café'), limit=20_000)
    return (
        'fff.multi_grep_literal_or.large.10k',
        'fff_multi_grep',
        'Warm-indexed literal OR matching for naming variants',
        measure(lambda: engine.multi_grep(request)),
        10_000,
        'indexed files',
    )


async def _close_all(engines: list[FffEngine]) -> None:
    await asyncio.gather(*(engine.close() for engine in engines))
