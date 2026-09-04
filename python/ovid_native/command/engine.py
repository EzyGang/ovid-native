import asyncio

from ovid_native import _native
from ovid_native._native_execution import NativeErrorTranslator, run_native_translated
from ovid_native.command.errors import (
    CommandCancelledError,
    CommandClosedError,
    CommandConfigurationError,
    CommandError,
    CommandExecutionError,
    CommandPathError,
)
from ovid_native.command.models import WorkspaceCommandRequest, WorkspaceCommandResult


_ERROR_TRANSLATOR = NativeErrorTranslator(
    {
        _native.NativeCommandConfigurationError: CommandConfigurationError,
        _native.NativeCommandPathError: CommandPathError,
        _native.NativeCommandExecutionError: CommandExecutionError,
        _native.NativeCommandCancelledError: CommandCancelledError,
        _native.NativeCommandClosedError: CommandClosedError,
    },
    fallback=CommandError,
)


class CommandEngine:
    def __init__(self, workspace: _native.NativeWorkspace) -> None:
        self._workspace = workspace
        self._condition = asyncio.Condition()
        self._active: dict[int, _native.NativeCommandCancellation] = {}
        self._next_execution = 0
        self._closed = False

    async def execute(self, request: WorkspaceCommandRequest) -> WorkspaceCommandResult:
        cancellation = _native.NativeCommandCancellation()
        async with self._condition:
            if self._closed:
                raise CommandClosedError('Workspace session is closed')
            execution = self._next_execution
            self._next_execution += 1
            self._active[execution] = cancellation
        try:
            native_request = _native.NativeCommandRequest(
                request.command,
                request.cwd,
                list(request.env.items()),
                request.timeout_seconds,
                request.max_output_bytes,
                cancellation,
            )
            result = await run_native_translated(
                lambda: _native.command_execute(self._workspace, native_request),
                cancellation=cancellation,
                translator=_ERROR_TRANSLATOR,
            )
            return WorkspaceCommandResult(
                output=result[0],
                exit_code=result[1],
                timed_out=result[2],
                truncated=result[3],
                wall_time_ms=result[4],
            )
        finally:
            async with self._condition:
                del self._active[execution]
                self._condition.notify_all()

    async def close(self) -> None:
        async with self._condition:
            self._closed = True
            for cancellation in self._active.values():
                cancellation.cancel()
            await self._condition.wait_for(lambda: not self._active)
