import hashlib
import json
import os
from pathlib import Path


SUITE_VERSION = 1
FIXTURE_VERSION = 1
SEARCH_FILE_COUNT = 10_000
AST_FILE_COUNT = 2_000
AST_APPLY_FILE_COUNT = 100

_FIXTURE_CONTRACT = {
    'suite_version': SUITE_VERSION,
    'fixture_version': FIXTURE_VERSION,
    'search_files': SEARCH_FILE_COUNT,
    'search_directories': 100,
    'ast_files': AST_FILE_COUNT,
    'ast_apply_files': AST_APPLY_FILE_COUNT,
    'search_sparse_interval': 100,
    'ast_sparse_interval': 10,
}


def build_fixtures(root: Path) -> None:
    root.mkdir(parents=True, exist_ok=True)
    _build_search_fixture(root / 'search')
    _build_ast_fixture(root / 'ast')
    manifest = {**_FIXTURE_CONTRACT, 'digest': _fixture_digest(root)}
    (root / 'manifest.json').write_text(json.dumps(manifest, indent=2, sort_keys=True) + '\n', encoding='utf-8')


def validate_fixtures(root: Path) -> str:
    manifest_path = root / 'manifest.json'
    try:
        manifest = json.loads(manifest_path.read_text(encoding='utf-8'))
    except (OSError, json.JSONDecodeError) as error:
        raise RuntimeError(f'Cannot read benchmark fixture manifest: {error}') from error

    if any(manifest.get(key) != value for key, value in _FIXTURE_CONTRACT.items()):
        raise RuntimeError('Benchmark fixture manifest does not match the suite contract')
    digest = manifest.get('digest')
    if (
        not isinstance(digest, str)
        or len(digest) != 64
        or any(character not in '0123456789abcdef' for character in digest)
    ):
        raise RuntimeError('Benchmark fixture manifest has an invalid content digest')

    required = (
        root / 'search/src/dir-000/file-000.py',
        root / 'search/src/dir-099/file-099.py',
        root / 'ast/src/module-0000.py',
        root / 'ast/src/module-1999.py',
        root / 'ast/apply/module-0099.py',
    )
    if not all(path.is_file() for path in required):
        raise RuntimeError('Benchmark fixture is incomplete')
    return digest


def _fixture_digest(root: Path) -> str:
    digest = hashlib.sha256()
    for path in sorted(root.rglob('*')):
        if not path.is_file() or path.name == 'manifest.json':
            continue
        relative = path.relative_to(root).as_posix().encode('utf-8')
        contents = path.read_bytes()
        digest.update(len(relative).to_bytes(8, byteorder='big'))
        digest.update(relative)
        digest.update(len(contents).to_bytes(8, byteorder='big'))
        digest.update(contents)
    return digest.hexdigest()


def _build_search_fixture(root: Path) -> None:
    source_root = root / 'src'
    for directory_index in range(100):
        directory = source_root / f'dir-{directory_index:03d}'
        directory.mkdir(parents=True)
        for file_index in range(100):
            index = directory_index * 100 + file_index
            sparse = 'needle value\n' if index % 100 == 0 else 'ordinary value\n'
            contents = f'common token {index}\n{sparse}unicode café {index}\n'
            path = directory / f'file-{file_index:03d}.py'
            path.write_text(contents, encoding='utf-8')
            os.utime(path, (1_700_000_000 + index, 1_700_000_000 + index))

    hot = root / 'hot/hot.txt'
    hot.parent.mkdir(parents=True)
    hot.write_text(''.join(f'needle hot {index}\n' for index in range(50_000)), encoding='utf-8')
    large = root / 'large/large.txt'
    large.parent.mkdir(parents=True)
    large.write_text('needle prefix\n' + ('large payload\n' * 100_000), encoding='utf-8')

    (root / '.gitignore').write_text('ignored/\n', encoding='utf-8')
    ignored = root / 'ignored/ignored.py'
    ignored.parent.mkdir()
    ignored.write_text('needle ignored\n', encoding='utf-8')
    (root / '.hidden.py').write_text('needle hidden\n', encoding='utf-8')
    dependency = root / 'node_modules/package/index.py'
    dependency.parent.mkdir(parents=True)
    dependency.write_text('needle dependency\n', encoding='utf-8')
    (root / 'binary.bin').write_bytes(b'needle\0binary')


def _build_ast_fixture(root: Path) -> None:
    source_root = root / 'src'
    source_root.mkdir(parents=True)
    for index in range(AST_FILE_COUNT):
        statement = 'print(value)\n' if index % 10 == 0 else 'result = value + 1\n'
        (source_root / f'module-{index:04d}.py').write_text(f'value = {index}\n{statement}', encoding='utf-8')

    apply_root = root / 'apply'
    apply_root.mkdir()
    for index in range(AST_APPLY_FILE_COUNT):
        (apply_root / f'module-{index:04d}.py').write_text(f'print({index})\n', encoding='utf-8')
