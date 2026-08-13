from ovid_core.models import BaseModel

from ovid_native import _native


class NativeCompatibilityError(RuntimeError):
    pass


class NativeRuntimeInfo(BaseModel):
    operating_system: str
    architecture: str
    api_version: int


_EXPECTED_NATIVE_API_VERSION = 5


def runtime_info() -> NativeRuntimeInfo:
    operating_system, architecture, api_version = _native.runtime_info()

    return NativeRuntimeInfo(
        operating_system=operating_system,
        architecture=architecture,
        api_version=api_version,
    )


def ensure_native_compatibility() -> None:
    actual_version = runtime_info().api_version

    if actual_version != _EXPECTED_NATIVE_API_VERSION:
        raise NativeCompatibilityError(
            f'ovid-native Python code expects native API {_EXPECTED_NATIVE_API_VERSION}, found {actual_version}'
        )
