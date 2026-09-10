import asyncio
import shutil
from pathlib import Path
from time import perf_counter
from typing import TYPE_CHECKING

from benchmarks.fixtures import AST_APPLY_FILE_COUNT
from ovid_native.ast.engine import AstEngine
from ovid_native.ast.models import (
    AstLimits,
    AstRewriteApplyRequest,
    AstRewriteOperation,
    AstRewritePreviewRequest,
    AstScanOptions,
    AstSearchRequest,
)


if TYPE_CHECKING:
    from benchmarks.scenarios import AsyncMeasure, BenchmarkCase


def ast_benchmark_cases(
    fixture_root: Path,
    work_root: Path,
    loop: asyncio.AbstractEventLoop,
    measure: AsyncMeasure,
) -> tuple[BenchmarkCase, ...]:
    ast_root = fixture_root / 'ast'
    ast_engine = AstEngine(
        root=ast_root,
        limits=AstLimits(
            max_matches=10_000,
            max_files=10_000,
            max_replacements=10_000,
            max_changed_files=2_000,
        ),
    )

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
    preview_request = AstRewritePreviewRequest(
        operations=(AstRewriteOperation(pattern='print($A)', replacement='log($A)'),),
        scan=AstScanOptions(paths=('src/**/*.py',)),
        language='python',
    )
    return (
        *(
            (name, 'ast', purpose, measure(lambda request=request: ast_engine.search(request)), count, 'files')
            for name, purpose, count, request in ast_cases
        ),
        (
            'ast.preview_sparse.2k',
            'ast',
            'Rewrite parsing, hashing, and edit construction',
            measure(lambda: ast_engine.preview_rewrite(preview_request)),
            2_000,
            'files',
        ),
        (
            'ast.apply.100',
            'ast',
            'Apply 100 staged file changes',
            apply_measure,
            AST_APPLY_FILE_COUNT,
            'files',
        ),
    )
