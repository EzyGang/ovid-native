from ovid_core.models import BaseModel

from ovid_native import _native


class NativeRuntimeInfo(BaseModel):
    operating_system: str
    architecture: str
    api_version: int


def runtime_info() -> NativeRuntimeInfo:
    operating_system, architecture, api_version = _native.runtime_info()

    return NativeRuntimeInfo(
        operating_system=operating_system,
        architecture=architecture,
        api_version=api_version,
    )
