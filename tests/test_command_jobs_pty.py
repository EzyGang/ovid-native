import asyncio
import shlex
import subprocess
import sys
import time
from pathlib import Path

from ovid_native.command.models import WorkspaceCommandJobRequest, WorkspaceCommandJobSnapshot
from ovid_native.workspace.models import WorkspaceCommandJobsProvider
from ovid_native.workspace.operations import WorkspaceOperation
from ovid_native.workspace.service import NativeWorkspaceSession


# PTY jobs run through cmd.exe on Windows and /bin/sh on Unix, so each platform quotes differently.
def _shell(*arguments: str) -> str:
    if sys.platform == 'win32':
        return subprocess.list2cmdline(arguments)
    return shlex.join(arguments)


def _python(script: str) -> str:
    return _shell(sys.executable, '-c', script)


def test_pty_job_sees_a_terminal(tmp_path: Path) -> None:
    async def run() -> WorkspaceCommandJobSnapshot:
        return await _run_to_end(tmp_path, _python('import sys; print(sys.stdin.isatty(), sys.stdout.isatty())'))

    finished = asyncio.run(run())

    assert 'True True' in finished.head + finished.tail
    assert (finished.status, finished.exit_code) == ('exited', 0)


def test_pty_write_answers_input(tmp_path: Path) -> None:
    async def run() -> str:
        session = _session(tmp_path)
        try:
            jobs = session.command_jobs
            started = await jobs.start(_request(_python('print("ready", flush=True); print(input().upper())')))
            await _read_until(jobs, started.job_id, 'ready')
            await jobs.write(started.job_id, 'ovid\r')
            finished = await jobs.wait(started.job_id, timeout_seconds=15, strip_ansi=True)
        finally:
            await session.close()
        return finished.head + finished.tail

    assert 'OVID' in asyncio.run(run())


def test_pty_resize_changes_terminal_size(tmp_path: Path) -> None:
    script = (
        'import os; size = os.get_terminal_size(); print(f"size {size.columns}x{size.lines}", flush=True); input(); '
        'size = os.get_terminal_size(); print(f"size {size.columns}x{size.lines}")'
    )

    async def run() -> tuple[str, str]:
        session = _session(tmp_path)
        try:
            jobs = session.command_jobs
            started = await jobs.start(_request(_python(script), cols=100, rows=30))
            before = await _read_until(jobs, started.job_id, 'size 100x30')
            await jobs.resize(started.job_id, cols=132, rows=43)
            await jobs.write(started.job_id, '\r')
            finished = await jobs.wait(started.job_id, timeout_seconds=15, strip_ansi=True)
        finally:
            await session.close()
        return before, finished.head + finished.tail

    before, after = asyncio.run(run())

    assert 'size 100x30' in before
    assert 'size 132x43' in after


def test_pty_job_keeps_quotes_cwd_env_and_native_exit_code(tmp_path: Path) -> None:
    nested = tmp_path / 'nested'
    nested.mkdir()
    script = 'import os, sys; print("quoted", os.getcwd(), os.environ["CUSTOM"]); sys.exit(7)'

    async def run() -> WorkspaceCommandJobSnapshot:
        return await _run_to_end(tmp_path, _python(script), cwd='nested', env={'CUSTOM': 'value'})

    finished = asyncio.run(run())

    assert f'quoted {nested.resolve()} value' in finished.head + finished.tail
    assert (finished.status, finished.exit_code) == ('exited', 7)


def test_pty_interrupt_stops_the_foreground_program(tmp_path: Path) -> None:
    async def run() -> WorkspaceCommandJobSnapshot:
        session = _session(tmp_path)
        try:
            jobs = session.command_jobs
            started = await jobs.start(_request(_python('import time; print("ready", flush=True); time.sleep(60)')))
            await _read_until(jobs, started.job_id, 'ready')
            await jobs.signal(started.job_id, 'interrupt')
            return await jobs.wait(started.job_id, timeout_seconds=15)
        finally:
            await session.close()

    finished = asyncio.run(run())

    assert finished.status == 'exited'
    assert finished.exit_code != 0


def _session(root: Path) -> NativeWorkspaceSession:
    return NativeWorkspaceSession(root=root, enabled_operations=frozenset((WorkspaceOperation.COMMAND_JOBS,)))


def _request(
    command: str, *, cols: int = 400, rows: int = 40, **fields: str | dict[str, str]
) -> WorkspaceCommandJobRequest:
    return WorkspaceCommandJobRequest.model_validate(
        {'command': command, 'pty': True, 'cols': cols, 'rows': rows, **fields}
    )


async def _run_to_end(root: Path, command: str, **fields: str | dict[str, str]) -> WorkspaceCommandJobSnapshot:
    session = _session(root)
    try:
        jobs = session.command_jobs
        started = await jobs.start(_request(command, **fields))
        return await jobs.wait(started.job_id, timeout_seconds=15, strip_ansi=True)
    finally:
        await session.close()


async def _read_until(jobs: WorkspaceCommandJobsProvider, job_id: int, text: str) -> str:
    output = ''
    offset = 0
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        snapshot = await jobs.poll(job_id, offset=offset, strip_ansi=True)
        output += snapshot.head + snapshot.tail
        offset = snapshot.next_offset
        if text in output:
            return output
        await asyncio.sleep(0.05)
    raise AssertionError(f'{text!r} did not appear in {output!r}')
