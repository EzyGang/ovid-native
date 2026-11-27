import asyncio
import contextvars
from collections.abc import Callable
from pathlib import Path
from typing import Protocol

from ovid_native import _native
from ovid_native.runtime import ensure_native_compatibility


class NativeCancellation(Protocol):
    def cancel(self) -> None: ...


class NativeErrorTranslator[Error: Exception]:
    def __init__(self, mapping: dict[type[Exception], type[Error]]) -> None:
        self.native_errors = tuple(mapping)
        self._mapping = mapping

    def __call__(self, error: Exception) -> Error:
        return self._mapping[type(error)](str(error))


def create_native_workspace(
    root: Path,
    *,
    error: type[Exception],
    state_dir: Path | None = None,
) -> _native.NativeWorkspace:
    ensure_native_compatibility()
    try:
        return _native.workspace_create(str(root), state_dir)
    except ValueError as cause:
        raise error(str(cause)) from cause


async def run_native[Result](
    operation: Callable[[], Result],
    *,
    cancellation: NativeCancellation | None = None,
) -> Result:
    loop = asyncio.get_running_loop()
    context = contextvars.copy_context()
    worker = loop.run_in_executor(None, context.run, operation)
    worker.add_done_callback(lambda completed: completed.exception() if not completed.cancelled() else None)
    try:
        return await asyncio.shield(worker)
    except asyncio.CancelledError as cancelled:
        if cancellation is not None:
            cancellation.cancel()
        while not worker.done():
            try:
                await asyncio.shield(worker)
            except asyncio.CancelledError:
                if cancellation is not None:
                    cancellation.cancel()
            except Exception:
                break
        try:
            worker.result()
        except Exception:
            pass
        raise cancelled


async def run_native_translated[Result, Error: Exception](
    operation: Callable[[], Result],
    *,
    translator: NativeErrorTranslator[Error],
    cancellation: NativeCancellation | None = None,
) -> Result:
    try:
        return await run_native(operation, cancellation=cancellation)
    except translator.native_errors as error:
        raise translator(error) from error
