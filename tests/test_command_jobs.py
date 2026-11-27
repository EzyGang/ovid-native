import asyncio
import ctypes
import os
import re
import shlex
import socket
import sys
import time
from pathlib import Path

import pytest

from ovid_native.command.errors import (
    CommandClosedError,
    CommandConfigurationError,
    CommandExecutionError,
    CommandJobLimitError,
    CommandJobNotFoundError,
)
from ovid_native.command.models import WorkspaceCommandJobRequest, WorkspaceCommandRequest
from ovid_native.workspace.errors import WorkspaceOperationUnavailableError
from ovid_native.workspace.models import WorkspaceCommandJobsProvider
from ovid_native.workspace.operations import WorkspaceOperation
from ovid_native.workspace.service import NativeWorkspaceSession


# `http.server` binds through `HTTPServer.server_bind`, which calls `socket.getfqdn`. That reverse lookup can take
# longer than the test deadline on macOS CI, so the server binds through `socketserver.TCPServer` directly.
_SERVER = (
    'import http.server, os, socketserver; print("pid", os.getpid(), flush=True); '
    'server = socketserver.TCPServer(("127.0.0.1", 0), http.server.SimpleHTTPRequestHandler); '
    'print("port", server.server_address[1], flush=True); server.serve_forever()'
)
_SLEEPER = 'import os, time; print("pid", os.getpid(), flush=True); time.sleep(60)'


def test_server_job_is_reachable_and_kill_frees_its_port(tmp_path: Path) -> None:
    async def run() -> None:
        session = _session(tmp_path)
        try:
            jobs = session.command_jobs
            started = await jobs.start(WorkspaceCommandJobRequest(command=_python(_SERVER, '-u')))
            output = await _read_until(jobs, started.job_id, r'port (\d+)')
            port = int(_match(output, r'port (\d+)'))
            pid = int(_match(output, r'pid (\d+)'))
            fetch = f'import urllib.request; print(urllib.request.urlopen("http://127.0.0.1:{port}/").status)'
            fetched = await session.command.execute(WorkspaceCommandRequest(command=_python(fetch)))

            await jobs.signal(started.job_id, 'kill')
            stopped = await jobs.wait(started.job_id, timeout_seconds=10)
        finally:
            await session.close()

        assert started.status == 'running'
        assert fetched.tail.strip() == '200'
        assert stopped.status == 'killed'
        assert stopped.exit_code is None
        assert _ends_within((pid,), seconds=2)
        with pytest.raises(OSError):
            socket.create_connection(('127.0.0.1', port), timeout=1).close()

    asyncio.run(run())


def test_poll_returns_new_bytes_and_overflow_keeps_the_head(tmp_path: Path) -> None:
    printed = bytes(97 + index % 26 for index in range(2_097_152))
    flood = 'import sys; sys.stdout.buffer.write(bytes(97 + index % 26 for index in range(2_097_152)))'

    async def run() -> None:
        session = _session(tmp_path)
        try:
            jobs = session.command_jobs
            echo = await jobs.start(WorkspaceCommandJobRequest(command=_python('print(input().upper())', '-u')))
            before = await jobs.poll(echo.job_id)
            await jobs.write(echo.job_id, 'ovid\n')
            await jobs.wait(echo.job_id, timeout_seconds=10)
            after = await jobs.poll(echo.job_id, offset=before.next_offset)
            again = await jobs.poll(echo.job_id, offset=after.next_offset)
            large = await jobs.start(WorkspaceCommandJobRequest(command=_python(flood)))
            overflowed = await jobs.wait(large.job_id, timeout_seconds=30)
            listed = await jobs.list()
        finally:
            await session.close()

        assert (before.head, before.tail, before.next_offset) == ('', '', 0)
        assert after.head.strip() == 'OVID'
        assert (after.status, after.exit_code) == ('exited', 0)
        assert (again.head, again.tail, again.next_offset) == ('', '', after.next_offset)
        assert overflowed.head.encode() == printed[:65_536]
        assert printed.endswith(overflowed.tail.encode())
        assert overflowed.truncated_before > 0
        assert overflowed.omitted_bytes == overflowed.truncated_before
        assert overflowed.next_offset == len(printed)
        assert [(job.job_id, job.tail, job.next_offset) for job in listed] == [
            (echo.job_id, '', after.next_offset),
            (large.job_id, '', len(printed)),
        ]

    asyncio.run(run())


def test_session_close_ends_every_job_quickly(tmp_path: Path) -> None:
    async def run() -> None:
        session = _session(tmp_path)
        jobs = session.command_jobs
        pids = []
        for _ in range(2):
            started = await jobs.start(WorkspaceCommandJobRequest(command=_python(_SLEEPER)))
            pids.append(int(_match(await _read_until(jobs, started.job_id, r'pid (\d+)'), r'pid (\d+)')))

        closing = time.monotonic()
        await session.close()
        assert _ends_within(pids, seconds=2 - (time.monotonic() - closing))
        with pytest.raises(CommandClosedError):
            await jobs.poll(1)

    asyncio.run(run())


def test_ninth_running_job_is_rejected_and_finished_jobs_do_not_count(tmp_path: Path) -> None:
    async def run() -> None:
        session = _session(tmp_path)
        try:
            jobs = session.command_jobs
            finished = await jobs.start(WorkspaceCommandJobRequest(command='exit 3'))
            assert (await jobs.wait(finished.job_id, timeout_seconds=10)).exit_code == 3
            running = [await jobs.start(WorkspaceCommandJobRequest(command=_python(_SLEEPER))) for _ in range(8)]

            with pytest.raises(CommandJobLimitError, match='at most 8'):
                await jobs.start(WorkspaceCommandJobRequest(command=_python(_SLEEPER)))
            await jobs.signal(running[0].job_id, 'terminate')
            replacement = await jobs.start(WorkspaceCommandJobRequest(command='true'))
        finally:
            await session.close()

        assert replacement.job_id == 10

    asyncio.run(run())


def test_job_timeout_and_typed_failures(tmp_path: Path) -> None:
    async def run() -> None:
        session = _session(tmp_path)
        try:
            jobs = session.command_jobs
            slow = await jobs.start(WorkspaceCommandJobRequest(command=_python(_SLEEPER), timeout_seconds=1))
            timed_out = await jobs.wait(slow.job_id, timeout_seconds=10, strip_ansi=True)
            pending = await jobs.start(WorkspaceCommandJobRequest(command=_python(_SLEEPER)))
            unfinished = await jobs.wait(pending.job_id, timeout_seconds=0)

            with pytest.raises(CommandJobNotFoundError, match='99'):
                await jobs.poll(99)
            with pytest.raises(CommandConfigurationError, match='offset'):
                await jobs.poll(pending.job_id, offset=-1)
            with pytest.raises(CommandConfigurationError, match='timeout_seconds'):
                await jobs.wait(pending.job_id, timeout_seconds=-1)
            with pytest.raises(CommandConfigurationError, match='cols'):
                await jobs.resize(pending.job_id, cols=0, rows=10)
            with pytest.raises(CommandConfigurationError, match='no terminal'):
                await jobs.resize(pending.job_id, cols=80, rows=24)
            with pytest.raises(CommandConfigurationError, match='interrupt'):
                await jobs.signal(pending.job_id, 'interrupt')
            with pytest.raises(CommandExecutionError, match='not running'):
                await jobs.write(slow.job_id, 'late')
        finally:
            await session.close()

        assert (timed_out.status, timed_out.exit_code) == ('timed_out', None)
        assert unfinished.status == 'running'

    asyncio.run(run())


def test_command_jobs_operation_can_be_disabled(tmp_path: Path) -> None:
    async def run() -> None:
        session = NativeWorkspaceSession(root=tmp_path, enabled_operations=frozenset((WorkspaceOperation.COMMAND,)))
        try:
            with pytest.raises(WorkspaceOperationUnavailableError, match='command_jobs'):
                _ = session.command_jobs
            assert (await session.command.execute(WorkspaceCommandRequest(command='exit 4'))).exit_code == 4
        finally:
            await session.close()

    asyncio.run(run())


def _session(root: Path) -> NativeWorkspaceSession:
    operations = frozenset((WorkspaceOperation.COMMAND, WorkspaceOperation.COMMAND_JOBS))
    return NativeWorkspaceSession(root=root, enabled_operations=operations)


def _python(script: str, *options: str) -> str:
    return shlex.join((sys.executable, *options, '-c', script))


async def _read_until(jobs: WorkspaceCommandJobsProvider, job_id: int, pattern: str) -> str:
    output = ''
    offset = 0
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        snapshot = await jobs.poll(job_id, offset=offset)
        output += snapshot.head + snapshot.tail
        offset = snapshot.next_offset
        if re.search(pattern, output):
            return output
        await asyncio.sleep(0.05)
    raise AssertionError(f'{pattern!r} did not appear in {output!r}')


def _match(output: str, pattern: str) -> str:
    match = re.search(pattern, output)
    assert match is not None
    return match.group(1)


def _ends_within(pids: tuple[int, ...] | list[int], *, seconds: float) -> bool:
    deadline = time.monotonic() + seconds
    while any(_is_alive(pid) for pid in pids):
        if time.monotonic() >= deadline:
            return False
        time.sleep(0.02)
    return True


def _is_alive(pid: int) -> bool:
    if sys.platform == 'win32':
        kernel = ctypes.windll.kernel32
        handle = kernel.OpenProcess(0x00100000, False, pid)
        if not handle:
            return False
        try:
            return kernel.WaitForSingleObject(handle, 0) != 0
        finally:
            kernel.CloseHandle(handle)
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    return True
