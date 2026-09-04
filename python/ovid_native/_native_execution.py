import asyncio
import contextvars
from collections.abc import Callable
from typing import Protocol


class NativeCancellation(Protocol):
    def cancel(self) -> None: ...


class NativeErrorTranslator[Error: Exception]:
    def __init__(
        self,
        mapping: dict[type[Exception], type[Error]],
        *,
        fallback: type[Error],
    ) -> None:
        self.native_errors = tuple(mapping)
        self._mapping = mapping
        self._fallback = fallback

    def __call__(self, error: Exception) -> Error:
        public_type = self._mapping.get(type(error), self._fallback)
        return public_type(str(error))


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
