import asyncio
import os
import time
from pathlib import Path

import pytest
from pytest_mock import MockerFixture

from ovid_native import _native
from ovid_native.ast import AstEngine, AstSearchRequest
from ovid_native.search import (
    GlobRequest,
    GrepRequest,
    SearchConfigurationError,
    SearchEngine,
    SearchLimitError,
    SearchLimits,
    SearchPathError,
    SearchPatternError,
    SearchReadError,
    SearchScanOptions,
)
from ovid_native.search.engine import _call_native, _translate_native
from ovid_native.search.errors import SearchCancelledError, SearchError


def test_glob_is_typed_ranked_bounded_and_workspace_safe(tmp_path: Path) -> None:
    source = tmp_path / 'src'
    source.mkdir()
    older = source / 'older.py'
    newer = source / 'newer.py'
    older.write_text('older\n')
    newer.write_text('newer\n')
    os.utime(older, (1, 1))
    os.utime(newer, (2, 2))
    (tmp_path / '.hidden.py').write_text('hidden\n')
    (tmp_path / '.gitignore').write_text('ignored.py\n')
    (tmp_path / 'ignored.py').write_text('ignored\n')
    engine = SearchEngine(root=tmp_path)

    ranked = asyncio.run(engine.glob(GlobRequest(patterns=('src',), file_type='file')))
    bounded = asyncio.run(engine.glob(GlobRequest(patterns=('.',), order='path', limit=1)))

    assert engine.root == tmp_path.resolve()
    assert engine.limits == SearchLimits()
    assert [match.path for match in ranked.matches] == ['src/newer.py', 'src/older.py']
    assert ranked.matches[0].modified_at is not None
    assert ranked.matches[0].size == len(newer.read_bytes())
    assert all(not match.path.startswith('/') for match in ranked.matches)
    assert bounded.completion == 'complete'
    assert bounded.truncated is True
    assert len(bounded.matches) == 1


def test_glob_directories_end_with_slash_and_path_errors_are_narrow(tmp_path: Path) -> None:
    (tmp_path / 'src').mkdir()
    engine = SearchEngine(root=tmp_path)

    result = asyncio.run(engine.glob(GlobRequest(patterns=('src',), file_type='directory', order='path')))

    assert result.matches[0].path == 'src/'
    assert result.matches[0].file_type == 'directory'
    assert result.matches[0].modified_at is None
    assert result.matches[0].size is None
    with pytest.raises(SearchPathError):
        asyncio.run(engine.glob(GlobRequest(patterns=('../outside',))))


def test_grep_reports_pagination_context_utf8_and_partial_coverage(tmp_path: Path) -> None:
    (tmp_path / 'a.txt').write_text('before\nnéedle one\nafter\nneedle two\n')
    (tmp_path / 'b.txt').write_text('needle three\n')
    (tmp_path / 'large.txt').write_text('needle trailing data')
    engine = SearchEngine(root=tmp_path)
    first = GrepRequest(
        pattern='n.edle',
        case_sensitive=False,
        file_limit=1,
        matches_per_file=1,
        context_before=1,
        context_after=1,
    )

    first_result = asyncio.run(engine.grep(first))
    second_result = asyncio.run(engine.grep(GrepRequest(pattern='n.edle', case_sensitive=False, file_offset=1)))
    prefix = asyncio.run(
        engine.grep(
            GrepRequest(
                pattern='needle',
                scan=SearchScanOptions(paths=('large.txt',)),
                max_file_bytes=6,
            )
        )
    )

    matched = first_result.files[0].matches[0]
    assert first_result.files[0].path == 'a.txt'
    assert matched.text == 'néedle'
    assert matched.range.start.line == 2
    assert matched.range.start.column == 1
    assert matched.range.start.byte_offset == (tmp_path / 'a.txt').read_bytes().index('néedle'.encode())
    assert matched.context_before[0].text == 'before'
    assert matched.context_after[0].text == 'after'
    assert first_result.files[0].total_matches == 2
    assert first_result.files[0].matches_truncated is True
    assert first_result.next_file_offset == 1
    assert second_result.files[0].path == 'b.txt'
    assert prefix.files[0].coverage.searched_bytes == 6
    assert prefix.files[0].coverage.total_bytes == len('needle trailing data')
    assert prefix.files[0].coverage.complete is False
    assert prefix.truncated is True


def test_grep_pattern_modes_binary_encoding_large_skip_and_line_truncation(tmp_path: Path) -> None:
    (tmp_path / 'patterns.txt').write_text('Foobar\n(\n')
    (tmp_path / 'binary.bin').write_bytes(b'nee\0dle')
    (tmp_path / 'encoding.txt').write_bytes(b'nee\xffdle')
    (tmp_path / 'large.txt').write_text('needle trailing data')
    engine = SearchEngine(root=tmp_path, limits=SearchLimits(max_line_characters=3))

    pcre = asyncio.run(engine.grep(GrepRequest(pattern='foo(?=bar)', case_sensitive=False)))
    auto = asyncio.run(engine.grep(GrepRequest(pattern='(', mode='auto')))
    skipped = asyncio.run(
        engine.grep(
            GrepRequest(
                pattern='needle',
                scan=SearchScanOptions(paths=('binary.bin', 'encoding.txt', 'large.txt')),
                max_file_bytes=6,
                large_file_mode='skip',
            )
        )
    )
    truncated_line = asyncio.run(
        engine.grep(GrepRequest(pattern='Foo', scan=SearchScanOptions(paths=('patterns.txt',))))
    )

    assert pcre.pattern_engine == 'pcre2'
    assert pcre.interpreted_as_literal is False
    assert auto.pattern_engine == 'rust'
    assert auto.interpreted_as_literal is True
    assert auto.files[0].matches[0].text == '('
    assert skipped.skipped_large_files == 3
    assert truncated_line.files[0].matches[0].line_text == 'Foo'
    assert truncated_line.files[0].matches[0].line_truncated is True


def test_ast_grep_and_glob_share_workspace_selection_policy(tmp_path: Path) -> None:
    (tmp_path / 'kept.py').write_text('print(1)\n')
    (tmp_path / '.hidden.py').write_text('print(2)\n')
    (tmp_path / '.gitignore').write_text('ignored.py\n')
    (tmp_path / 'ignored.py').write_text('print(3)\n')
    node_modules = tmp_path / 'node_modules'
    node_modules.mkdir()
    (node_modules / 'dependency.py').write_text('print(4)\n')
    ast = AstEngine(root=tmp_path)
    search = SearchEngine(root=tmp_path)

    ast_result = asyncio.run(ast.search(AstSearchRequest(pattern='print($A)')))
    grep_result = asyncio.run(search.grep(GrepRequest(pattern='print')))
    glob_result = asyncio.run(search.glob(GlobRequest(patterns=('*.py',), file_type='file', order='path')))

    ast_paths = {match.path for match in ast_result.matches}
    grep_paths = {file.path for file in grep_result.files}
    glob_paths = {match.path for match in glob_result.matches}
    assert ast_paths == grep_paths == glob_paths == {'kept.py'}


def test_search_engine_enforces_every_safety_ceiling(tmp_path: Path) -> None:
    engine = SearchEngine(
        root=tmp_path,
        limits=SearchLimits(
            max_glob_results=1,
            max_grep_files=1,
            max_matches_per_file=1,
            max_file_bytes=1,
            max_context_lines=1,
            max_timeout_seconds=1,
        ),
    )
    invalid_requests = (
        GlobRequest(limit=2),
        GlobRequest(limit=1, timeout_seconds=2),
        GrepRequest(pattern='x', file_limit=2, max_file_bytes=1, timeout_seconds=1),
        GrepRequest(pattern='x', matches_per_file=2, max_file_bytes=1, timeout_seconds=1),
        GrepRequest(pattern='x', max_file_bytes=2, timeout_seconds=1),
        GrepRequest(pattern='x', context_before=2, max_file_bytes=1, timeout_seconds=1),
        GrepRequest(pattern='x', context_after=2, max_file_bytes=1, timeout_seconds=1),
        GrepRequest(pattern='x', max_file_bytes=1, timeout_seconds=2),
    )
    for request in invalid_requests:
        with pytest.raises(SearchLimitError):
            if isinstance(request, GlobRequest):
                asyncio.run(engine.glob(request))
            else:
                asyncio.run(engine.grep(request))


def test_search_configuration_pattern_and_native_errors_are_narrow(tmp_path: Path) -> None:
    with pytest.raises(SearchConfigurationError):
        SearchEngine(root=tmp_path / 'missing')
    file_root = tmp_path / 'file'
    file_root.write_text('not a directory')
    with pytest.raises(SearchConfigurationError):
        SearchEngine(root=file_root)

    engine = SearchEngine(root=tmp_path)
    with pytest.raises(SearchPatternError):
        asyncio.run(engine.grep(GrepRequest(pattern='(')))

    mappings = (
        (_native.NativeSearchConfigurationError('x'), SearchConfigurationError),
        (_native.NativeSearchPathError('x'), SearchPathError),
        (_native.NativeSearchPatternError('x'), SearchPatternError),
        (_native.NativeSearchLimitError('x'), SearchLimitError),
        (_native.NativeSearchCancelledError('x'), SearchCancelledError),
        (_native.NativeSearchReadError('x'), SearchReadError),
    )
    for native_error, public_type in mappings:
        translated = _translate_native(native_error)
        assert isinstance(translated, public_type)
        assert str(translated) == 'x'
    assert type(_translate_native(Exception('x'))) is SearchError


def test_native_search_runs_off_event_loop_and_preserves_causes(tmp_path: Path, mocker: MockerFixture) -> None:
    engine = SearchEngine(root=tmp_path)

    def slow_grep(
        workspace: _native.NativeWorkspace,
        request: _native.NativeGrepRequest,
    ) -> _native.NativeGrepResult:
        del workspace, request
        time.sleep(0.05)
        return ([], 'rust', False, 'complete', 0, 0, True, 0, 0, 0, None, False)

    mocker.patch('ovid_native.search.engine._native.search_grep', side_effect=slow_grep)

    async def scenario() -> None:
        task = asyncio.create_task(engine.grep(GrepRequest(pattern='x')))
        await asyncio.sleep(0.01)
        assert not task.done()
        await task
        cancelled = asyncio.create_task(engine.grep(GrepRequest(pattern='x')))
        await asyncio.sleep(0.01)
        cancelled.cancel()
        with pytest.raises(asyncio.CancelledError):
            await cancelled

    asyncio.run(scenario())

    failure = mocker.Mock(side_effect=_native.NativeSearchReadError('read failed'))
    cancellation = mocker.Mock()
    with pytest.raises(SearchReadError) as captured:
        asyncio.run(_call_native(failure, cancellation=cancellation))
    assert isinstance(captured.value.__cause__, _native.NativeSearchReadError)
