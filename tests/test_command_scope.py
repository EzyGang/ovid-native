import asyncio
import shlex
import subprocess
import sys
import time
from pathlib import Path

import pytest

from ovid_native.command.models import WorkspaceCommandRequest, WorkspaceCommandResult
from ovid_native.workspace.operations import WorkspaceOperation
from ovid_native.workspace.service import NativeWorkspaceSession


_GRANDCHILD = (
    'import sys, time; from pathlib import Path; '
    "Path(f'{sys.argv[1]}.started').write_text('started'); "
    "time.sleep(1.5); Path(f'{sys.argv[1]}.alive').write_text('alive')"
)
_PARENT = (
    'import subprocess, sys; '
    f"children = [subprocess.Popen([sys.executable, '-c', {_GRANDCHILD!r}, name]) for name in ('first', 'second')]; "
    '[child.wait() for child in children]'
)


def test_commands_from_two_sessions_run_at_the_same_time(tmp_path: Path) -> None:
    async def run() -> None:
        first = _session(tmp_path)
        second = _session(tmp_path)
        request = WorkspaceCommandRequest(command=_python('import time; time.sleep(2)'))
        started = time.monotonic()
        try:
            results = await asyncio.gather(first.command.execute(request), second.command.execute(request))
        finally:
            await first.close()
            await second.close()

        assert time.monotonic() - started < 3
        assert [result.exit_code for result in results] == [0, 0]

    asyncio.run(run())


@pytest.mark.parametrize(
    ('command_seconds', 'timeout_seconds', 'cancel'),
    [(1, 30, False), (5, 1, False), (5, 30, True)],
    ids=('completes', 'times-out', 'is-cancelled'),
)
def test_host_process_survives_the_command(
    tmp_path: Path, command_seconds: int, timeout_seconds: int, cancel: bool
) -> None:
    async def run() -> None:
        session = _session(tmp_path)
        task = asyncio.create_task(
            session.command.execute(
                WorkspaceCommandRequest(
                    command=_python(f'import time; time.sleep({command_seconds})'), timeout_seconds=timeout_seconds
                )
            )
        )
        await asyncio.sleep(0.3)
        assert not task.done()
        host_child = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(60)'])
        try:
            if cancel:
                task.cancel()
            await asyncio.gather(task, return_exceptions=True)
            await asyncio.sleep(0.3)

            assert host_child.poll() is None
        finally:
            host_child.kill()
            host_child.wait()
            await session.close()

    asyncio.run(run())


@pytest.mark.parametrize(
    'command',
    (
        f'{shlex.quote(sys.executable)} -c {shlex.quote(_PARENT)}',
        f'{shlex.quote(sys.executable)} -c {shlex.quote(_GRANDCHILD)} first & '
        f'{shlex.quote(sys.executable)} -c {shlex.quote(_GRANDCHILD)} second & wait',
    ),
    ids=('descendants', 'background-jobs'),
)
def test_timed_out_command_leaves_no_process_from_its_tree(tmp_path: Path, command: str) -> None:
    result = _execute(tmp_path, command, timeout_seconds=1)
    time.sleep(2)

    assert result.timed_out
    assert sorted(path.name for path in tmp_path.iterdir()) == ['first.started', 'second.started']


def test_pipeline_of_external_commands_passes_output(tmp_path: Path) -> None:
    producer = _python("print('alpha')")
    consumer = _python('import sys; print(sys.stdin.read().strip().upper())')

    result = _execute(tmp_path, f'{producer} | {consumer}')

    assert result.tail.strip() == 'ALPHA'
    assert result.exit_code == 0


def _session(root: Path) -> NativeWorkspaceSession:
    return NativeWorkspaceSession(root=root, enabled_operations=frozenset((WorkspaceOperation.COMMAND,)))


def _python(script: str) -> str:
    return f'{shlex.quote(sys.executable)} -c {shlex.quote(script)}'


def _execute(root: Path, command: str, *, timeout_seconds: float = 30) -> WorkspaceCommandResult:
    async def run() -> WorkspaceCommandResult:
        session = _session(root)
        try:
            return await session.command.execute(
                WorkspaceCommandRequest(command=command, timeout_seconds=timeout_seconds)
            )
        finally:
            await session.close()

    return asyncio.run(run())
