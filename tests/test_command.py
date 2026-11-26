import asyncio
import os
import shlex
import sys
from pathlib import Path
from threading import Event

import pytest
from pydantic import ValidationError
from pytest_mock import MockerFixture

from ovid_native import _native
from ovid_native._native_execution import run_native_translated
from ovid_native.command.engine import _ERROR_TRANSLATOR
from ovid_native.command.errors import (
    CommandCancelledError,
    CommandClosedError,
    CommandConfigurationError,
    CommandError,
    CommandExecutionError,
    CommandPathError,
)
from ovid_native.command.models import WorkspaceCommandRequest
from ovid_native.workspace.errors import WorkspaceOperationUnavailableError
from ovid_native.workspace.operations import WorkspaceOperation
from ovid_native.workspace.service import NativeWorkspaceSession


def test_native_workspace_command_inherits_environment_and_maps_result(
    tmp_path: Path,
    mocker: MockerFixture,
) -> None:
    async def run() -> None:
        mocker.patch.dict(os.environ, {'OVID_COMMAND_INHERITED': 'parent'}, clear=False)
        nested = tmp_path / 'nested'
        nested.mkdir()
        session = NativeWorkspaceSession(root=tmp_path, enabled_operations=frozenset((WorkspaceOperation.COMMAND,)))
        try:
            result = await session.command.execute(
                WorkspaceCommandRequest(
                    command='printf "%s|%s|%s" "$OVID_COMMAND_INHERITED" "$CUSTOM" "$PWD"; exit 7',
                    cwd=str(nested),
                    env={'CUSTOM': 'override'},
                )
            )
        finally:
            await session.close()

        assert result.head == ''
        assert result.tail == f'parent|override|{nested.resolve()}'
        assert result.exit_code == 7
        assert not result.timed_out
        assert result.omitted_bytes == 0
        assert result.wall_time_ms >= 0

    asyncio.run(run())


@pytest.mark.parametrize('max_head_bytes', (16_384, 0))
def test_native_workspace_command_returns_head_tail_and_omitted_bytes(tmp_path: Path, max_head_bytes: int) -> None:
    printed = bytes(97 + index % 26 for index in range(204_800)).decode()
    script = 'import sys; sys.stdout.buffer.write(bytes(97 + index % 26 for index in range(204_800)))'

    async def run() -> None:
        session = NativeWorkspaceSession(root=tmp_path, enabled_operations=frozenset((WorkspaceOperation.COMMAND,)))
        try:
            result = await session.command.execute(
                WorkspaceCommandRequest(
                    command=f'{shlex.quote(sys.executable)} -c {shlex.quote(script)}',
                    max_output_bytes=65_536,
                    max_head_bytes=max_head_bytes,
                )
            )
        finally:
            await session.close()

        assert result.head == printed[:max_head_bytes]
        assert result.tail == printed[-(65_536 - max_head_bytes) :]
        assert result.omitted_bytes == 204_800 - 65_536

    asyncio.run(run())


@pytest.mark.parametrize('character', ('é', '€', '😀'))
def test_native_workspace_command_never_splits_characters_at_cuts(tmp_path: Path, character: str) -> None:
    printed = f'a{character}xxxxxxxx{character}b'

    async def run() -> None:
        session = NativeWorkspaceSession(root=tmp_path, enabled_operations=frozenset((WorkspaceOperation.COMMAND,)))
        try:
            result = await session.command.execute(
                WorkspaceCommandRequest(command=f"printf '{printed}'", max_output_bytes=4, max_head_bytes=2)
            )
        finally:
            await session.close()

        assert (result.head, result.tail) == ('a', 'b')
        assert result.omitted_bytes == len(printed.encode()) - 2

    asyncio.run(run())


def test_command_request_rejects_head_larger_than_output() -> None:
    with pytest.raises(ValidationError, match='max_head_bytes cannot exceed max_output_bytes'):
        WorkspaceCommandRequest(command='true', max_output_bytes=10, max_head_bytes=11)

    assert WorkspaceCommandRequest(command='true', max_output_bytes=10, max_head_bytes=10).max_head_bytes == 10


def test_native_workspace_command_times_out_and_cancels_without_late_side_effects(tmp_path: Path) -> None:
    async def run() -> None:
        timeout_marker = tmp_path / 'timeout-marker'
        cancellation_marker = tmp_path / 'cancellation-marker'
        session = NativeWorkspaceSession(root=tmp_path, enabled_operations=frozenset((WorkspaceOperation.COMMAND,)))
        try:
            timed_out = await session.command.execute(
                WorkspaceCommandRequest(
                    command=_delayed_write_command(timeout_marker.name, delay_seconds=1.25),
                    timeout_seconds=1,
                    max_output_bytes=10,
                )
            )
            await asyncio.sleep(0.35)
            task = asyncio.create_task(
                session.command.execute(
                    WorkspaceCommandRequest(
                        command=_delayed_write_command(cancellation_marker.name, delay_seconds=0.25)
                    )
                )
            )
            await asyncio.sleep(0.05)
            task.cancel()
            with pytest.raises(asyncio.CancelledError):
                await task
            await asyncio.sleep(0.35)
        finally:
            await session.close()

        assert timed_out.timed_out
        assert timed_out.exit_code is None
        assert not timeout_marker.exists()
        assert not cancellation_marker.exists()

    asyncio.run(run())


def _delayed_write_command(path: str, *, delay_seconds: float) -> str:
    script = f'import time; from pathlib import Path; time.sleep({delay_seconds}); Path({path!r}).write_text("done")'
    return f'{shlex.quote(sys.executable)} -c {shlex.quote(script)}'


def test_workspace_close_cancels_active_command_before_returning(tmp_path: Path) -> None:
    async def run() -> None:
        marker = tmp_path / 'close-marker'
        session = NativeWorkspaceSession(root=tmp_path, enabled_operations=frozenset((WorkspaceOperation.COMMAND,)))
        provider = session.command
        task = asyncio.create_task(
            provider.execute(WorkspaceCommandRequest(command=_delayed_write_command(marker.name, delay_seconds=0.25)))
        )
        await asyncio.sleep(0.05)

        await session.close()
        with pytest.raises(CommandCancelledError):
            await task
        await asyncio.sleep(0.35)

        assert not marker.exists()
        with pytest.raises(CommandClosedError, match='closed'):
            await provider.execute(WorkspaceCommandRequest(command='true'))

    asyncio.run(run())


def test_command_provider_rejects_unavailable_and_closed_workspace(tmp_path: Path) -> None:
    async def run() -> None:
        disabled = NativeWorkspaceSession(root=tmp_path, enabled_operations=frozenset())
        with pytest.raises(WorkspaceOperationUnavailableError, match='command'):
            _ = disabled.command
        await disabled.close()

        session = NativeWorkspaceSession(root=tmp_path, enabled_operations=frozenset((WorkspaceOperation.COMMAND,)))
        provider = session.command
        await session.close()
        with pytest.raises(CommandClosedError, match='closed'):
            await provider.execute(WorkspaceCommandRequest(command='true'))

    asyncio.run(run())


@pytest.mark.parametrize(
    ('native_error', 'public_type'),
    [
        (_native.NativeCommandConfigurationError('configuration'), CommandConfigurationError),
        (_native.NativeCommandPathError('path'), CommandPathError),
        (_native.NativeCommandExecutionError('execution'), CommandExecutionError),
        (_native.NativeCommandCancelledError('cancelled'), CommandCancelledError),
        (_native.NativeCommandClosedError('closed'), CommandClosedError),
    ],
)
def test_command_error_translation(native_error: Exception, public_type: type[CommandError]) -> None:
    translated = _ERROR_TRANSLATOR(native_error)

    assert isinstance(translated, public_type)
    assert str(translated) == str(native_error)


@pytest.mark.parametrize('cooperative', (False, True))
def test_repeated_cancellation_waits_for_native_worker(cooperative: bool) -> None:
    async def run() -> None:
        loop = asyncio.get_running_loop()
        started = asyncio.Event()
        release = Event()
        completed = Event()

        def operation() -> None:
            loop.call_soon_threadsafe(started.set)
            release.wait(timeout=5)
            completed.set()

        cancellation = _native.NativeCommandCancellation() if cooperative else None
        task = asyncio.create_task(
            run_native_translated(operation, cancellation=cancellation, translator=_ERROR_TRANSLATOR)
        )
        try:
            await asyncio.wait_for(started.wait(), timeout=5)
            task.cancel()
            await asyncio.sleep(0)
            task.cancel()
            await asyncio.sleep(0)
            assert not task.done()
        finally:
            release.set()

        with pytest.raises(asyncio.CancelledError):
            await task
        assert completed.is_set()

    asyncio.run(run())
