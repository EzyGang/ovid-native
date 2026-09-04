import asyncio
import os
import shlex
import sys
from pathlib import Path

import pytest
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

        assert result.output == f'parent|override|{nested.resolve()}'
        assert result.exit_code == 7
        assert not result.timed_out
        assert not result.truncated
        assert result.wall_time_ms >= 0

    asyncio.run(run())


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
        (RuntimeError('unknown'), CommandError),
    ],
)
def test_command_error_translation(native_error: Exception, public_type: type[CommandError]) -> None:
    translated = _ERROR_TRANSLATOR(native_error)

    assert isinstance(translated, public_type)
    assert str(translated) == str(native_error)


def test_native_call_translates_known_errors() -> None:
    async def run() -> None:
        cancellation = _native.NativeCommandCancellation()

        def fail() -> None:
            raise _native.NativeCommandExecutionError('failed')

        with pytest.raises(CommandExecutionError, match='failed'):
            await run_native_translated(
                fail,
                cancellation=cancellation,
                translator=_ERROR_TRANSLATOR,
            )

    asyncio.run(run())
