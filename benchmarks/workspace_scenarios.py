import asyncio
from pathlib import Path
from time import perf_counter
from typing import TYPE_CHECKING

from ovid_native.files.models import WorkspaceFileReadRequest
from ovid_native.workspace.rehydration import WorkspaceRehydrationRequest
from ovid_native.workspace.service import NativeWorkspaceSession


if TYPE_CHECKING:
    from benchmarks.scenarios import BenchmarkCase


def workspace_benchmark_cases(fixture_root: Path, loop: asyncio.AbstractEventLoop) -> tuple[BenchmarkCase, ...]:
    ast_root = fixture_root / 'ast'
    setup = NativeWorkspaceSession(root=ast_root)
    reads = [
        loop.run_until_complete(setup.files.read_file(WorkspaceFileReadRequest(path=f'src/module-{index:04d}.py')))
        for index in range(200)
    ]
    loop.run_until_complete(setup.close())
    requests = tuple(
        WorkspaceRehydrationRequest(
            path=read.path,
            content_sha256=read.observation.content_sha256,
            visible_ranges=tuple((line.start, line.end) for line in read.observation.visible_ranges),
            complete_presentation=read.observation.complete_presentation,
        )
        for read in reads
        if read.observation is not None
    )

    def rehydrate_measure(loops: int) -> float:
        total = 0.0
        for _ in range(loops):
            workspace = NativeWorkspaceSession(root=ast_root)
            started = perf_counter()
            report = loop.run_until_complete(workspace.observations.rehydrate(requests))
            total += perf_counter() - started
            loop.run_until_complete(workspace.close())
            if report.restored != len(reads):
                raise RuntimeError(f'Rehydration restored {report.restored} of {len(reads)} receipts')
        return total

    return (
        (
            'workspace.rehydrate.200',
            'workspace',
            'Digest verification and ledger restore for 200 receipts in a fresh session',
            rehydrate_measure,
            len(reads),
            'receipts',
        ),
    )
