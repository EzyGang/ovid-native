import asyncio
from pathlib import Path

import pytest
from pytest_mock import MockerFixture

from ovid_native import _native
from ovid_native.discovery.errors import FileDiscoveryConfigurationError, FileDiscoveryEncodingError
from ovid_native.discovery.models import NamedFileDiscoveryRequest
from ovid_native.discovery.operations import discover_named_files, find_ancestor_entry, read_text_files


def test_standalone_discovery_functions_cover_ancestor_reads_and_named_walk(tmp_path: Path) -> None:
    repository = tmp_path / 'repository'
    nested = repository / 'packages' / 'app'
    nested.mkdir(parents=True)
    (repository / '.git').write_text('gitdir: elsewhere', encoding='utf-8')
    first = repository / 'AGENTS.md'
    first.write_text('repository rules', encoding='utf-8')
    missing = repository / 'missing.md'
    deeper = nested / 'src'
    deeper.mkdir()
    (deeper / 'AGENTS.md').write_text('deeper rules', encoding='utf-8')

    ancestor = asyncio.run(find_ancestor_entry(start=nested, name='.git'))
    files = asyncio.run(read_text_files((first, missing)))
    result = asyncio.run(discover_named_files(root=nested, request=NamedFileDiscoveryRequest(filename='AGENTS.md')))

    assert ancestor == repository.resolve()
    assert tuple((file.path, file.content) for file in files) == ((first, 'repository rules'),)
    assert result.paths == ('src/AGENTS.md',)
    assert result.completion == 'complete'


def test_standalone_discovery_translates_configuration_and_encoding_errors(
    tmp_path: Path,
    mocker: MockerFixture,
) -> None:
    with pytest.raises(FileDiscoveryConfigurationError, match='must be one file name'):
        asyncio.run(
            discover_named_files(
                root=tmp_path,
                request=NamedFileDiscoveryRequest(filename='../AGENTS.md'),
            )
        )

    mocker.patch(
        'ovid_native.discovery.operations._native.discovery_read_text_files',
        side_effect=_native.NativeDiscoveryEncodingError('invalid UTF-8'),
    )
    with pytest.raises(FileDiscoveryEncodingError, match='invalid UTF-8'):
        asyncio.run(read_text_files((tmp_path / 'AGENTS.md',)))
