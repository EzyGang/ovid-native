import asyncio
import shutil
from collections.abc import Awaitable, Callable, Sequence
from dataclasses import dataclass
from pathlib import Path
from time import perf_counter

from benchmarks.fff_scenarios import fff_benchmark_cases
from benchmarks.fixtures import AST_APPLY_FILE_COUNT
from ovid_native.ast import (
    AstEngine,
    AstLimits,
    AstRewriteApplyRequest,
    AstRewriteOperation,
    AstRewritePreviewRequest,
    AstScanOptions,
    AstSearchRequest,
)
from ovid_native.search import GlobRequest, GrepRequest, SearchEngine, SearchLimits, SearchScanOptions


@dataclass(frozen=True, slots=True)
class Scenario:
    name: str
    operation: str
    purpose: str
    measure: Callable[[int], float]
    work_items: int
    work_unit: str


@dataclass(frozen=True, slots=True)
class ScenarioSuite:
    cases: tuple[Scenario, ...]
    close: Callable[[], None]


def scenarios(fixture_root: Path, work_root: Path) -> ScenarioSuite:
    search_root = fixture_root / 'search'
    ast_root = fixture_root / 'ast'
    search_engine = SearchEngine(
        root=search_root,
        limits=SearchLimits(
            max_scan_files=20_000,
            max_glob_results=20_000,
            max_grep_files=20_000,
            max_grep_matches=100_000,
            max_matches_per_file=100_000,
            max_file_bytes=4 * 1024 * 1024,
        ),
    )
    ast_engine = AstEngine(
        root=ast_root,
        limits=AstLimits(
            max_matches=10_000,
            max_files=10_000,
            max_replacements=10_000,
            max_changed_files=2_000,
        ),
    )
    loop = asyncio.new_event_loop()

    def async_measure(operation: Callable[[], Awaitable[object]]) -> Callable[[int], float]:
        def measure(loops: int) -> float:
            started = perf_counter()
            for _ in range(loops):
                loop.run_until_complete(operation())
            return perf_counter() - started

        return measure

    def apply_measure(loops: int) -> float:
        apply_root = work_root / 'ast-apply'
        source_root = ast_root / 'apply'
        total = 0.0
        for _ in range(loops):
            if apply_root.exists():
                shutil.rmtree(apply_root)
            shutil.copytree(source_root, apply_root)
            apply_engine = AstEngine(
                root=apply_root,
                limits=AstLimits(
                    max_matches=1_000,
                    max_files=1_000,
                    max_replacements=1_000,
                    max_changed_files=AST_APPLY_FILE_COUNT,
                ),
            )
            preview = loop.run_until_complete(
                apply_engine.preview_rewrite(
                    AstRewritePreviewRequest(
                        operations=(AstRewriteOperation(pattern='print($A)', replacement='log($A)'),),
                        scan=AstScanOptions(paths=('*.py',)),
                        language='python',
                    )
                )
            )
            started_apply = perf_counter()
            loop.run_until_complete(apply_engine.apply_rewrite(AstRewriteApplyRequest(proposal_id=preview.proposal_id)))
            total += perf_counter() - started_apply
        return total

    glob_cases = (
        (
            'glob.exact_file.10k',
            'Exact path lookup and selection pruning',
            1,
            GlobRequest(patterns=('src/dir-099/file-099.py',), order='path', limit=10_000),
        ),
        (
            'glob.directory.10k',
            'Directory selection and subtree traversal',
            100,
            GlobRequest(patterns=('src/dir-050',), order='path', limit=10_000),
        ),
        (
            'glob.recursive_path.10k',
            'Recursive glob path ordering',
            10_000,
            GlobRequest(patterns=('**/*.py',), order='path', limit=20_000),
        ),
        (
            'glob.recursive_mtime.10k',
            'Metadata retrieval and modified-time ranking',
            10_000,
            GlobRequest(patterns=('**/*.py',), order='modified_desc', limit=20_000),
        ),
    )
    grep_cases = (
        (
            'grep.literal_miss.10k',
            'Literal absence and complete read cost',
            10_000,
            GrepRequest(
                pattern='not-present-anywhere',
                mode='literal',
                scan=SearchScanOptions(paths=('src/**/*.py',)),
                file_limit=20_000,
            ),
        ),
        (
            'grep.literal_sparse.10k',
            'Literal matching and file pagination',
            10_000,
            GrepRequest(
                pattern='needle value',
                mode='literal',
                scan=SearchScanOptions(paths=('src/**/*.py',)),
                file_limit=20_000,
            ),
        ),
        (
            'grep.literal_dense.10k',
            'Dense match collection',
            10_000,
            GrepRequest(
                pattern='common token',
                mode='literal',
                scan=SearchScanOptions(paths=('src/**/*.py',)),
                file_limit=20_000,
            ),
        ),
        (
            'grep.pcre2_sparse.10k',
            'Embedded PCRE2 path',
            10_000,
            GrepRequest(
                pattern='needle(?= value)',
                mode='regex',
                scan=SearchScanOptions(paths=('src/**/*.py',)),
                file_limit=20_000,
            ),
        ),
        (
            'grep.hot_file.1mib',
            'Many matches and line-index construction',
            1,
            GrepRequest(
                pattern='needle hot',
                mode='literal',
                scan=SearchScanOptions(paths=('hot/hot.txt',)),
                file_limit=10,
                matches_per_file=100_000,
            ),
        ),
        (
            'grep.prefix_large_file',
            'Bounded prefix search',
            1,
            GrepRequest(
                pattern='needle prefix',
                mode='literal',
                scan=SearchScanOptions(paths=('large/large.txt',)),
                file_limit=10,
                max_file_bytes=64 * 1024,
                large_file_mode='prefix',
            ),
        ),
    )
    ast_cases = (
        (
            'ast.search_miss.2k',
            'Parse and traversal with no structural matches',
            2_000,
            AstSearchRequest(
                pattern='await $A',
                scan=AstScanOptions(paths=('src/**/*.py',)),
                language='python',
                limit=10_000,
            ),
        ),
        (
            'ast.search_sparse.2k',
            'Structural match construction',
            2_000,
            AstSearchRequest(
                pattern='print($A)',
                scan=AstScanOptions(paths=('src/**/*.py',)),
                language='python',
                limit=10_000,
                include_captures=False,
            ),
        ),
        (
            'ast.search_captures.2k',
            'Capture extraction and mapping',
            2_000,
            AstSearchRequest(
                pattern='print($A)',
                scan=AstScanOptions(paths=('src/**/*.py',)),
                language='python',
                limit=10_000,
                include_captures=True,
            ),
        ),
    )

    benchmark_cases: list[Scenario] = []
    for name, purpose, work_items, request in glob_cases:
        benchmark_cases.append(
            Scenario(
                name,
                'glob',
                purpose,
                async_measure(lambda request=request: search_engine.glob(request)),
                work_items,
                'files',
            )
        )
    for name, purpose, work_items, request in grep_cases:
        benchmark_cases.append(
            Scenario(
                name,
                'grep',
                purpose,
                async_measure(lambda request=request: search_engine.grep(request)),
                work_items,
                'files',
            )
        )
    for name, purpose, work_items, request in ast_cases:
        benchmark_cases.append(
            Scenario(
                name,
                'ast',
                purpose,
                async_measure(lambda request=request: ast_engine.search(request)),
                work_items,
                'files',
            )
        )
    preview_request = AstRewritePreviewRequest(
        operations=(AstRewriteOperation(pattern='print($A)', replacement='log($A)'),),
        scan=AstScanOptions(paths=('src/**/*.py',)),
        language='python',
    )
    benchmark_cases.append(
        Scenario(
            'ast.preview_sparse.2k',
            'ast',
            'Rewrite parsing, hashing, and edit construction',
            async_measure(lambda: ast_engine.preview_rewrite(preview_request)),
            2_000,
            'files',
        )
    )
    benchmark_cases.append(
        Scenario(
            'ast.apply.100',
            'ast',
            'Apply 100 staged file changes',
            apply_measure,
            AST_APPLY_FILE_COUNT,
            'files',
        )
    )
    fff_cases, close = fff_benchmark_cases(fixture_root, loop, async_measure)
    benchmark_cases.extend(Scenario(*case) for case in fff_cases)

    return ScenarioSuite(cases=tuple(benchmark_cases), close=close)


def select_scenarios(all_scenarios: Sequence[Scenario], patterns: Sequence[str]) -> tuple[Scenario, ...]:
    if not patterns:
        return tuple(all_scenarios)

    selected = tuple(scenario for scenario in all_scenarios if any(pattern in scenario.name for pattern in patterns))
    if not selected:
        raise RuntimeError(f'No benchmark scenarios match: {", ".join(patterns)}')
    return selected
