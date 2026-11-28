from pathlib import Path

from ovid_native import _native
from ovid_native.runtime import ensure_native_compatibility


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
