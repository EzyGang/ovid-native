from ovid_native.runtime import runtime_info


def test_runtime_info_reports_native_build() -> None:
    info = runtime_info()

    assert info.api_version == 3
    assert info.operating_system in {'linux', 'macos', 'windows'}
    assert info.architecture
    assert info.ast_grep_version == '0.45.1'
