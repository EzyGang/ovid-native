from pathlib import Path
from typing import TYPE_CHECKING

from ovid_native.search.engine import SearchEngine
from ovid_native.search.models import GlobRequest, GrepRequest, SearchLimits, SearchScanOptions


if TYPE_CHECKING:
    from benchmarks.scenarios import AsyncMeasure, BenchmarkCase


def search_benchmark_cases(fixture_root: Path, measure: AsyncMeasure) -> tuple[BenchmarkCase, ...]:
    search_root = fixture_root / 'search'
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
    return (
        *(
            (name, 'glob', purpose, measure(lambda request=request: search_engine.glob(request)), count, 'files')
            for name, purpose, count, request in glob_cases
        ),
        *(
            (name, 'grep', purpose, measure(lambda request=request: search_engine.grep(request)), count, 'files')
            for name, purpose, count, request in grep_cases
        ),
    )
