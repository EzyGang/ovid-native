import asyncio
from collections.abc import Callable
from typing import Protocol


class NativeCancellation(Protocol):
    def cancel(self) -> None: ...


async def run_native[Result](
    operation: Callable[[], Result],
    *,
    cancellation: NativeCancellation | None = None,
) -> Result:
    try:
        return await asyncio.to_thread(operation)
    except asyncio.CancelledError:
        if cancellation is not None:
            cancellation.cancel()
        raise
