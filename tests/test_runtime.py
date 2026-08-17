import pytest
from pytest_mock import MockerFixture

from ovid_native import _native
from ovid_native.runtime import NativeCompatibilityError, ensure_native_compatibility, runtime_info


def test_runtime_info_reports_native_build() -> None:
    info = runtime_info()

    assert info.api_version == 7
    assert info.operating_system in {'linux', 'macos', 'windows'}
    assert info.architecture


def test_native_compatibility_rejects_a_mismatched_extension(mocker: MockerFixture) -> None:
    mocker.patch.object(_native, 'runtime_info', return_value=('macos', 'arm64', 999))

    with pytest.raises(NativeCompatibilityError, match='expects native API 7, found 999'):
        ensure_native_compatibility()
