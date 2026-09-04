from collections.abc import Sequence
from pathlib import Path

from ovid_native import _native
from ovid_native._native_execution import NativeErrorTranslator, run_native_translated
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


_ERROR_TRANSLATOR = NativeErrorTranslator(
    {
        _native.NativeDiscoveryConfigurationError: FileDiscoveryConfigurationError,
        _native.NativeDiscoveryPathError: FileDiscoveryPathError,
        _native.NativeDiscoveryEncodingError: FileDiscoveryEncodingError,
        _native.NativeDiscoveryReadError: FileDiscoveryReadError,
        _native.NativeDiscoveryCancelledError: FileDiscoveryCancelledError,
    },
    fallback=FileDiscoveryError,
)


async def find_ancestor_entry(*, start: Path, name: str) -> Path | None:
    ensure_native_compatibility()
    value = await run_native_translated(
        lambda: _native.discovery_find_ancestor_entry(str(start), name),
        translator=_ERROR_TRANSLATOR,
    )
    return None if value is None else Path(value)


async def read_text_files(paths: Sequence[Path]) -> tuple[TextFile, ...]:
    ensure_native_compatibility()
    cancellation = _native.NativeDiscoveryCancellation()
    files = await run_native_translated(
        lambda: _native.discovery_read_text_files([str(path) for path in paths], cancellation),
        cancellation=cancellation,
        translator=_ERROR_TRANSLATOR,
    )
    return tuple(TextFile(path=Path(path), content=content) for path, content in files)


async def discover_named_files(
    *,
    root: Path,
    request: NamedFileDiscoveryRequest,
) -> NamedFileDiscoveryResult:
    ensure_native_compatibility()
    cancellation = _native.NativeDiscoveryCancellation()
    paths, completion = await run_native_translated(
        lambda: _native.discovery_find_named_files(
            str(root),
            request.filename,
            request.max_depth,
            request.limit,
            request.timeout_seconds,
            cancellation,
        ),
        cancellation=cancellation,
        translator=_ERROR_TRANSLATOR,
    )
    return NamedFileDiscoveryResult(paths=tuple(paths), completion=completion)
