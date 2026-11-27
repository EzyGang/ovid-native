import asyncio
from collections.abc import Callable

from ovid_native import _native
from ovid_native._native_execution import NativeErrorTranslator, run_native, run_native_translated
from ovid_native.command.errors import (
    CommandCancelledError,
    CommandClosedError,
    CommandConfigurationError,
    CommandExecutionError,
    CommandJobLimitError,
    CommandJobNotFoundError,
    CommandPathError,
)
from ovid_native.command.models import (
    CommandJobSignal,
    WorkspaceCommandJobRequest,
    WorkspaceCommandJobSnapshot,
    WorkspaceCommandRequest,
    WorkspaceCommandResult,
)


_ERROR_TRANSLATOR = NativeErrorTranslator(
    {
        _native.NativeCommandConfigurationError: CommandConfigurationError,
        _native.NativeCommandPathError: CommandPathError,
        _native.NativeCommandExecutionError: CommandExecutionError,
        _native.NativeCommandCancelledError: CommandCancelledError,
        _native.NativeCommandClosedError: CommandClosedError,
        _native.NativeCommandJobLimitError: CommandJobLimitError,
        _native.NativeCommandJobNotFoundError: CommandJobNotFoundError,
    }
)


class CommandEngine:
    def __init__(self, workspace: _native.NativeWorkspace) -> None:
        self._workspace = workspace
        self._jobs = _native.NativeCommandJobs(workspace)
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
                request.max_head_bytes,
                cancellation,
            )
            result = await run_native_translated(
                lambda: _native.command_execute(self._workspace, native_request),
                cancellation=cancellation,
                translator=_ERROR_TRANSLATOR,
            )
            return WorkspaceCommandResult(
                head=result[0],
                tail=result[1],
                omitted_bytes=result[2],
                exit_code=result[3],
                timed_out=result[4],
                wall_time_ms=result[5],
            )
        finally:
            async with self._condition:
                del self._active[execution]
                self._condition.notify_all()

    async def start(self, request: WorkspaceCommandJobRequest) -> WorkspaceCommandJobSnapshot:
        native_request = _native.NativeCommandJobRequest(
            request.command,
            request.cwd,
            list(request.env.items()),
            request.timeout_seconds,
            (request.cols, request.rows) if request.pty else None,
            request.max_buffer_bytes,
        )
        return _snapshot(await self._job_operation(lambda: self._jobs.start(native_request)))

    async def poll(self, job_id: int, *, offset: int = 0, strip_ansi: bool = False) -> WorkspaceCommandJobSnapshot:
        _require_range('offset', offset, 0, _MAX_OFFSET)
        return _snapshot(await self._job_operation(lambda: self._jobs.poll(job_id, offset, strip_ansi)))

    async def write(self, job_id: int, data: str) -> None:
        await self._job_operation(lambda: self._jobs.write(job_id, data))

    async def resize(self, job_id: int, *, cols: int, rows: int) -> None:
        _require_range('cols', cols, 1, 1_000)
        _require_range('rows', rows, 1, 1_000)
        await self._job_operation(lambda: self._jobs.resize(job_id, cols, rows))

    async def signal(self, job_id: int, signal: CommandJobSignal) -> None:
        await self._job_operation(lambda: self._jobs.signal(job_id, signal))

    async def wait(
        self,
        job_id: int,
        *,
        timeout_seconds: float,
        offset: int = 0,
        strip_ansi: bool = False,
    ) -> WorkspaceCommandJobSnapshot:
        _require_range('timeout_seconds', timeout_seconds, 0, 3_600)
        _require_range('offset', offset, 0, _MAX_OFFSET)
        cancellation = _native.NativeCommandCancellation()
        values = await run_native_translated(
            lambda: self._jobs.wait(job_id, offset, strip_ansi, timeout_seconds, cancellation),
            cancellation=cancellation,
            translator=_ERROR_TRANSLATOR,
        )
        return _snapshot(values)

    async def list(self) -> tuple[WorkspaceCommandJobSnapshot, ...]:
        return tuple(_snapshot(values) for values in await self._job_operation(self._jobs.list_jobs))

    async def close(self) -> None:
        async with self._condition:
            self._closed = True
            for cancellation in self._active.values():
                cancellation.cancel()
            await self._condition.wait_for(lambda: not self._active)
        await run_native(self._jobs.close)

    async def _job_operation[Result](self, operation: Callable[[], Result]) -> Result:
        return await run_native_translated(operation, translator=_ERROR_TRANSLATOR)


_MAX_OFFSET = 2**64 - 1


def _require_range(name: str, value: float, low: float, high: float) -> None:
    if not low <= value <= high:
        raise CommandConfigurationError(f'{name} must be between {low} and {high}')


def _snapshot(values: _native.NativeCommandJobSnapshot) -> WorkspaceCommandJobSnapshot:
    return WorkspaceCommandJobSnapshot(
        job_id=values[0],
        status=values[1],
        exit_code=values[2],
        head=values[3],
        tail=values[4],
        omitted_bytes=values[5],
        next_offset=values[6],
        truncated_before=values[7],
        wall_time_ms=values[8],
    )
