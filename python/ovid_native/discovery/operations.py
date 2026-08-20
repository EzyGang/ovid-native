from collections.abc import Callable, Sequence
from pathlib import Path

from ovid_native import _native
from ovid_native._native_execution import run_native
from ovid_native.discovery.errors import (
    FileDiscoveryCancelledError,
    FileDiscoveryConfigurationError,
    FileDiscoveryEncodingError,
    FileDiscoveryError,
    FileDiscoveryPathError,
    FileDiscoveryReadError,
)
from ovid_native.discovery.models import NamedFileDiscoveryRequest, NamedFileDiscoveryResult, TextFile
from ovid_native.runtime import ensure_native_compatibility


_NATIVE_ERRORS: tuple[type[Exception], ...] = (
    _native.NativeDiscoveryConfigurationError,
    _native.NativeDiscoveryPathError,
    _native.NativeDiscoveryReadError,
    _native.NativeDiscoveryEncodingError,
    _native.NativeDiscoveryCancelledError,
)


async def find_ancestor_entry(*, start: Path, name: str) -> Path | None:
    ensure_native_compatibility()
    value = await _call_native(lambda: _native.discovery_find_ancestor_entry(str(start), name))
    return None if value is None else Path(value)


async def read_text_files(paths: Sequence[Path]) -> tuple[TextFile, ...]:
    ensure_native_compatibility()
    cancellation = _native.NativeDiscoveryCancellation()
    files = await _call_native(
        lambda: _native.discovery_read_text_files([str(path) for path in paths], cancellation),
        cancellation=cancellation,
    )
    return tuple(TextFile(path=Path(path), content=content) for path, content in files)


async def discover_named_files(
    *,
    root: Path,
    request: NamedFileDiscoveryRequest,
) -> NamedFileDiscoveryResult:
    ensure_native_compatibility()
    cancellation = _native.NativeDiscoveryCancellation()
    paths, completion = await _call_native(
        lambda: _native.discovery_find_named_files(
            str(root),
            request.filename,
            request.max_depth,
            request.limit,
            request.timeout_seconds,
            cancellation,
        ),
        cancellation=cancellation,
    )
    return NamedFileDiscoveryResult(paths=tuple(paths), completion=completion)


async def _call_native[Result](
    operation: Callable[[], Result],
    *,
    cancellation: _native.NativeDiscoveryCancellation | None = None,
) -> Result:
    try:
        return await run_native(operation, cancellation=cancellation)
    except _NATIVE_ERRORS as error:
        raise _translate_native_error(error) from error


def _translate_native_error(error: Exception) -> FileDiscoveryError:
    mapping: dict[type[Exception], type[FileDiscoveryError]] = {
        _native.NativeDiscoveryConfigurationError: FileDiscoveryConfigurationError,
        _native.NativeDiscoveryPathError: FileDiscoveryPathError,
        _native.NativeDiscoveryEncodingError: FileDiscoveryEncodingError,
        _native.NativeDiscoveryReadError: FileDiscoveryReadError,
        _native.NativeDiscoveryCancelledError: FileDiscoveryCancelledError,
    }
    return mapping[type(error)](str(error))
