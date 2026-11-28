import asyncio
import ctypes
import hashlib
import os
import shlex
import subprocess
import sys
import threading
import time
from collections.abc import Callable
from pathlib import Path

import pytest
from pydantic import ValidationError

from ovid_native._native_execution import create_native_workspace
from ovid_native.command.engine import CommandEngine
from ovid_native.command.errors import CommandClosedError
from ovid_native.command.models import WorkspaceCommandJobRequest, WorkspaceCommandRequest, WorkspaceReapReport
from ovid_native.workspace.errors import WorkspaceConfigurationError
from ovid_native.workspace.operations import WorkspaceOperation
from ovid_native.workspace.service import NativeWorkspaceSession


# The command leader starts a grandchild, writes both process IDs, and sleeps. On Linux, the leader dies with the
# host through PR_SET_PDEATHSIG, and only a reap stops the grandchild.
_LEADER = (
    'import os, subprocess, sys, time; from pathlib import Path; '
    "child = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(600)']); "
    "Path(sys.argv[1]).write_text(f'{os.getpid()} {child.pid}'); time.sleep(600)"
)
_HOST = """
import asyncio, sys
from pathlib import Path
from ovid_native.command.models import WorkspaceCommandJobRequest, WorkspaceCommandRequest
from ovid_native.workspace.operations import WorkspaceOperation
from ovid_native.workspace.service import NativeWorkspaceSession

async def main() -> None:
    root, state, job_command, foreground_command = sys.argv[1:]
    operations = frozenset((WorkspaceOperation.COMMAND, WorkspaceOperation.COMMAND_JOBS))
    session = NativeWorkspaceSession(root=Path(root), state_dir=Path(state), enabled_operations=operations)
    await session.command_jobs.start(WorkspaceCommandJobRequest(command=job_command, owner_tag='job-call'))
    await session.command.execute(
        WorkspaceCommandRequest(command=foreground_command, owner_tag='command-call', timeout_seconds=3600)
    )

asyncio.run(main())
"""


def test_reap_stops_the_processes_of_a_killed_host(tmp_path: Path) -> None:
    host, (job, command) = _start_host(tmp_path)
    leaders, grandchildren = (job[0], command[0]), (job[1], command[1])
    host.kill()
    host.wait()

    if sys.platform == 'win32':
        assert _ends_within((*leaders, *grandchildren), seconds=10)
    if sys.platform == 'linux':  # [UNVERIFIED locally]
        assert _ends_within(leaders, seconds=10)
    report = _reap(tmp_path)

    stopped = {process.pid for process in report.stopped}
    if sys.platform == 'win32':
        assert stopped == set()
    else:  # [UNVERIFIED locally]
        assert stopped >= set(grandchildren)
        assert {process.owner_tag for process in report.stopped} == {'job-call', 'command-call'}
        assert all(len(process.command_hash_prefix) == 12 for process in report.stopped)
    assert _ends_within((*leaders, *grandchildren), seconds=10)
    assert not list((tmp_path / 'state' / 'processes').glob('*.record'))


def test_reap_skips_the_processes_of_a_live_host(tmp_path: Path) -> None:
    host, (job, command) = _start_host(tmp_path)
    try:
        report = _reap(tmp_path)

        assert report == WorkspaceReapReport(skipped_live_owners=1)
        assert all(_is_alive(pid) for pid in (*job, *command))
    finally:
        host.kill()
        host.wait()
        _reap(tmp_path)


def test_reap_never_signals_a_process_that_reuses_a_recorded_pid(tmp_path: Path) -> None:
    unrelated = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(600)'])
    try:
        processes = tmp_path / 'state' / 'processes'
        processes.mkdir(parents=True)
        boot = Path('/proc/sys/kernel/random/boot_id').read_text().strip() if sys.platform == 'linux' else ''
        digest = hashlib.sha256(b'sleep').hexdigest()
        (processes / f'{unrelated.pid}-1.record').write_text(
            f'ovid-process-record 1\nhost\t{unrelated.pid}\t1\t{boot}\n'
            f'scope\tcommand\t\t{digest}\t0\t{unrelated.pid}\t{unrelated.pid}:1\n'
        )

        report = _reap(tmp_path)

        assert report == WorkspaceReapReport()
        assert unrelated.poll() is None
        assert not list(processes.iterdir())
    finally:
        unrelated.kill()
        unrelated.wait()


def test_jobs_outlive_the_threads_that_start_them(tmp_path: Path) -> None:
    session = _session(tmp_path, state_dir=None)
    pty_marker, background_marker = tmp_path / 'pty.done', tmp_path / 'background.done'
    pty_command = (subprocess.list2cmdline if sys.platform == 'win32' else shlex.join)(
        (sys.executable, '-c', _write_later(pty_marker))
    )
    requests = (
        WorkspaceCommandJobRequest(command=pty_command, pty=True),
        WorkspaceCommandJobRequest(command=f'{shlex.join((sys.executable, "-c", _write_later(background_marker)))} &'),
    )
    try:
        for request in requests:
            thread = threading.Thread(target=lambda request=request: asyncio.run(session.command_jobs.start(request)))
            thread.start()
            thread.join()

        assert _wait_for(lambda: pty_marker.exists() and background_marker.exists(), seconds=20)
    finally:
        asyncio.run(session.close())


def test_engine_reaps_nothing_without_a_state_directory_and_rejects_a_closed_workspace(tmp_path: Path) -> None:
    async def run() -> None:
        native = create_native_workspace(tmp_path, error=WorkspaceConfigurationError)
        engine = CommandEngine(native)
        try:
            assert await engine.reap_orphans() == WorkspaceReapReport()
        finally:
            await engine.close()
        session = _session(tmp_path, state_dir=tmp_path / 'state')
        await session.close()
        with pytest.raises(CommandClosedError):
            await session.reap_orphans()

    asyncio.run(run())


@pytest.mark.parametrize('owner_tag', ('', 'x' * 129))
def test_requests_reject_invalid_owner_tags(owner_tag: str) -> None:
    with pytest.raises(ValidationError):
        WorkspaceCommandRequest(command='true', owner_tag=owner_tag)
    with pytest.raises(ValidationError):
        WorkspaceCommandJobRequest(command='true', owner_tag=owner_tag)


def _start_host(root: Path) -> tuple[subprocess.Popen[bytes], tuple[tuple[int, int], tuple[int, int]]]:
    job_file, command_file = root / 'job.pids', root / 'command.pids'
    commands = (shlex.join((sys.executable, '-c', _LEADER, str(path))) for path in (job_file, command_file))
    state = root / 'state'
    host = subprocess.Popen([sys.executable, '-c', _HOST, str(root), str(state), *commands])
    try:
        assert _wait_for(lambda: job_file.exists() and command_file.exists(), seconds=30)
        job, command = (tuple(int(pid) for pid in path.read_text().split()) for path in (job_file, command_file))
        assert _wait_for(lambda: _recorded(state, (*job, *command)), seconds=10)
    except BaseException:
        host.kill()
        host.wait()
        raise
    return host, ((job[0], job[1]), (command[0], command[1]))


def _recorded(state: Path, pids: tuple[int, ...]) -> bool:
    text = ''.join(path.read_text() for path in (state / 'processes').glob('*.record'))
    return all(f'{pid}:' in text for pid in pids)


def _reap(root: Path) -> WorkspaceReapReport:
    async def run() -> WorkspaceReapReport:
        session = _session(root, state_dir=root / 'state')
        try:
            return await session.reap_orphans()
        finally:
            await session.close()

    return asyncio.run(run())


def _session(root: Path, *, state_dir: Path | None) -> NativeWorkspaceSession:
    operations = frozenset((WorkspaceOperation.COMMAND, WorkspaceOperation.COMMAND_JOBS))
    return NativeWorkspaceSession(root=root, state_dir=state_dir, enabled_operations=operations)


def _wait_for(condition: Callable[[], bool], *, seconds: float) -> bool:
    deadline = time.monotonic() + seconds
    while not condition():
        if time.monotonic() >= deadline:
            return False
        time.sleep(0.05)
    return True


def _write_later(marker: Path) -> str:
    return f'import time; from pathlib import Path; time.sleep(2); Path({str(marker)!r}).write_text("x")'


def _ends_within(pids: tuple[int, ...], *, seconds: float) -> bool:
    return _wait_for(lambda: not any(_is_alive(pid) for pid in pids), seconds=seconds)


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
    if sys.platform == 'linux':
        try:
            state = Path(f'/proc/{pid}/stat').read_text().rpartition(')')[2].split()[0]
        except FileNotFoundError, ProcessLookupError:
            return False
        return state not in {'Z', 'X'}
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    return True
