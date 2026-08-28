import asyncio
from pathlib import Path
from typing import cast

import pytest
from ovid_core.tools.base import ToolExecutionContext

from ovid_native.files import (
    ReadLineRange,
    ReadTool,
    WorkspaceDirectoryTreeLine,
    WorkspaceReadDirectoryTreeResult,
    WorkspaceReadFileResult,
    WorkspaceReadRequest,
)
from ovid_native.files.read_results import render_directory_read, render_file_read
from ovid_native.files.read_targets import (
    MAX_READ_RANGES,
    MAX_READ_TARGETS,
    format_selector,
    parse_read_target,
    plan_read_target,
    split_read_targets,
)
from ovid_native.workspace.errors import WorkspacePathError, WorkspaceReadError
from ovid_native.workspace.evidence import capture_source_presentation
from ovid_native.workspace.policy import WorkspacePolicy
from ovid_native.workspace.service import NativeWorkspaceSession


def tool_context() -> ToolExecutionContext[None]:
    return cast('ToolExecutionContext[None]', None)


def test_read_tool_supports_inline_ranges_and_bounded_continuations(tmp_path: Path) -> None:
    source = tmp_path / 'source.txt'
    source.write_text(''.join(f'line-{number}\n' for number in range(1, 401)))
    workspace = NativeWorkspaceSession(root=tmp_path)
    read_tool = ReadTool[None](provider=workspace.files)

    bounded = asyncio.run(read_tool.execute(tool_context(), WorkspaceReadRequest(path='source.txt:5')))
    bounded_lines = cast(str, bounded.content).splitlines()
    assert bounded_lines[1] == '5:line-5'
    assert bounded_lines[-2] == '304:line-304'
    assert bounded_lines[-1] == '[Read limit reached. Continue with source.txt:305-400]'
    assert bounded.metadata['returned_lines'] == 300
    assert bounded.metadata['truncation'] == {
        'reasons': ['line_limit'],
        'continuations': ['source.txt:305-400'],
    }

    for selector in ('5-7', '5+3'):
        selected = asyncio.run(read_tool.execute(tool_context(), WorkspaceReadRequest(path=f'source.txt:{selector}')))
        assert cast(str, selected.content).splitlines()[1:] == ['5:line-5', '6:line-6', '7:line-7']
        assert selected.metadata['truncated'] is False

    disjoint = asyncio.run(read_tool.execute(tool_context(), WorkspaceReadRequest(path='source.txt:2-3,399-400')))
    assert cast(str, disjoint.content).splitlines()[1:] == [
        '2:line-2',
        '3:line-3',
        '…',
        '399:line-399',
        '400:line-400',
    ]
    unmatched = asyncio.run(read_tool.execute(tool_context(), WorkspaceReadRequest(path='source.txt:999')))
    assert cast(str, unmatched.content).splitlines() == [
        '[source.txt]',
        '[No lines found for 999; file has 400 lines]',
    ]
    assert unmatched.metadata['unmatched_ranges'] == [[999, None]]
    asyncio.run(workspace.close())


def test_read_tool_combines_targets_and_keeps_partial_failures(tmp_path: Path) -> None:
    (tmp_path / 'one.txt').write_text('one\nsecond\n')
    (tmp_path / 'two.txt').write_text('first\ntwo\n')
    (tmp_path / 'literal;name.txt').write_text('literal\n')
    workspace = NativeWorkspaceSession(root=tmp_path)
    read_tool = ReadTool[None](provider=workspace.files)

    result = asyncio.run(
        read_tool.execute(
            tool_context(),
            WorkspaceReadRequest(path='one.txt:1; missing.txt; two.txt:2-2'),
        )
    )
    content = cast(str, result.content)
    assert '[one.txt]\n1:one' in content
    assert '[Could not read missing.txt: cannot inspect missing.txt:' in content
    assert '[two.txt]\n2:two' in content
    assert result.metadata['kind'] == 'multi'
    assert result.metadata['target_count'] == 3
    assert result.metadata['successful_targets'] == 2
    assert result.metadata['failed_targets'] == 1

    literal = asyncio.run(read_tool.execute(tool_context(), WorkspaceReadRequest(path='literal;name.txt')))
    assert cast(str, literal.content).splitlines() == ['[literal;name.txt]', '1:literal']
    assert literal.metadata['kind'] == 'file'
    with pytest.raises(WorkspacePathError):
        asyncio.run(read_tool.execute(tool_context(), WorkspaceReadRequest(path='missing-only.txt')))
    asyncio.run(workspace.close())


def test_read_tool_reports_byte_limits_and_empty_files(tmp_path: Path) -> None:
    (tmp_path / 'source.txt').write_text('one\ntwo\nthree\nfour\nfive\n')
    (tmp_path / 'empty.txt').write_text('')
    workspace = NativeWorkspaceSession(
        root=tmp_path,
        policy=WorkspacePolicy(max_read_bytes=3),
    )
    read_tool = ReadTool[None](provider=workspace.files)

    partial = asyncio.run(read_tool.execute(tool_context(), WorkspaceReadRequest(path='source.txt:1-3')))
    assert cast(str, partial.content).splitlines() == [
        '[source.txt]',
        '1:one',
        '[Read limit reached. Continue with source.txt:2-3]',
    ]
    assert partial.metadata['truncation'] == {
        'reasons': ['byte_limit'],
        'continuations': ['source.txt:2-3'],
    }

    disjoint = asyncio.run(read_tool.execute(tool_context(), WorkspaceReadRequest(path='source.txt:1-2,4-5')))
    assert disjoint.metadata['truncation'] == {
        'reasons': ['byte_limit'],
        'continuations': ['source.txt:2-2,4-5'],
    }

    workspace.policy.update(max_read_bytes=2)
    oversized_line = asyncio.run(read_tool.execute(tool_context(), WorkspaceReadRequest(path='source.txt:1-1')))
    assert cast(str, oversized_line.content).splitlines() == [
        '[source.txt]',
        '[Line 1 exceeds the read byte limit and was not rendered]',
    ]
    empty = asyncio.run(read_tool.execute(tool_context(), WorkspaceReadRequest(path='empty.txt')))
    assert cast(str, empty.content).splitlines() == ['[empty.txt]', '[empty file]']
    asyncio.run(workspace.close())


def test_read_target_parser_rejects_ambiguous_or_excessive_inputs() -> None:
    assert split_read_targets('one; two') == ('one', 'two')
    with pytest.raises(WorkspaceReadError, match='cannot be empty'):
        split_read_targets('one;;two')
    with pytest.raises(WorkspaceReadError, match='at most'):
        split_read_targets(';'.join(f'{index}.txt' for index in range(MAX_READ_TARGETS + 1)))

    assert parse_read_target('name:value').path == 'name:value'
    assert parse_read_target('name:').path == 'name:'
    with pytest.raises(WorkspaceReadError, match='does not include a path'):
        parse_read_target(':1')
    with pytest.raises(WorkspaceReadError, match='either inline'):
        parse_read_target('name:1', (ReadLineRange(start=1, end=1),))
    with pytest.raises(WorkspaceReadError, match='Invalid read selector'):
        parse_read_target('name:1+x')
    with pytest.raises(WorkspaceReadError, match='cannot precede'):
        parse_read_target('name:5-3')
    with pytest.raises(WorkspaceReadError, match='cannot overlap'):
        parse_read_target('name:1-3,3-5')
    with pytest.raises(WorkspaceReadError, match='cannot overlap'):
        parse_read_target('name:1-,5-6')
    selector = ','.join(str(index * 2 + 1) for index in range(MAX_READ_RANGES + 1))
    with pytest.raises(WorkspaceReadError, match='at most'):
        parse_read_target(f'name:{selector}')

    empty_plan = plan_read_target(parse_read_target('name:1-3'), 0)
    assert empty_plan.ranges == ()
    assert format_selector('name', (ReadLineRange(start=5),)) == 'name:5'


def test_read_tool_renders_nested_directories_as_a_tree(tmp_path: Path) -> None:
    (tmp_path / 'src' / 'pkg').mkdir(parents=True)
    (tmp_path / 'src' / 'pkg' / 'module.py').write_text('value = 1\n')
    (tmp_path / 'src' / 'root.py').write_text('value = 2\n')
    workspace = NativeWorkspaceSession(root=tmp_path)
    read_tool = ReadTool[None](provider=workspace.files)

    result = asyncio.run(
        read_tool.execute(
            tool_context(),
            WorkspaceReadRequest(path='src', directory_depth=2),
        )
    )

    assert cast(str, result.content).splitlines() == [
        '[src]',
        'pkg/',
        '  module.py',
        'root.py',
    ]
    assert result.metadata['tree_lines'] == 3
    assert result.metadata['limited_directories'] == 0
    asyncio.run(workspace.close())


def test_read_renderers_cover_directory_limits_and_deferred_ranges() -> None:
    lines = tuple(
        WorkspaceDirectoryTreeLine(
            depth=0,
            name=f'entry-{index}',
            kind='directory' if index == 1 else 'file',
        )
        for index in range(1, 302)
    )
    result = WorkspaceReadDirectoryTreeResult(
        path='.',
        lines=lines,
        child_limit=12,
        scanned_entries=301,
        limited_directories=0,
        omitted_entries=0,
        scan_truncated=False,
    )
    open_target = plan_read_target(parse_read_target('.:1'), 3_000)
    rendered = render_directory_read(result, open_target)
    assert 'entry-1/' in rendered.content
    assert rendered.content.endswith('[Read limit reached. Continue with .:301-301]')

    unmatched = render_directory_read(
        result.model_copy(update={'lines': lines[:1], 'scanned_entries': 1}),
        plan_read_target(parse_read_target('.:999'), 3_000),
    )
    assert unmatched.content.endswith('[No entries found for 999; directory tree has 1 lines]')

    hierarchical = render_directory_read(
        WorkspaceReadDirectoryTreeResult(
            path='src',
            lines=(
                WorkspaceDirectoryTreeLine(depth=0, name='pkg', kind='directory'),
                *(WorkspaceDirectoryTreeLine(depth=1, name=f'child-{index:02}', kind='file') for index in range(11)),
                WorkspaceDirectoryTreeLine(depth=1, name='', kind='omitted', omitted_entries=2),
                WorkspaceDirectoryTreeLine(depth=1, name='child-13', kind='file'),
            ),
            child_limit=12,
            scanned_entries=15,
            limited_directories=1,
            omitted_entries=2,
            scan_truncated=False,
        ),
        plan_read_target(parse_read_target('src'), 3_000),
    )
    assert hierarchical.content.splitlines()[:4] == [
        '[src]',
        'pkg/',
        '  child-00',
        '  child-01',
    ]
    assert '  … 2 more' in hierarchical.content
    assert hierarchical.content.endswith(
        '[Some directories were abbreviated to 12 children. Read a subdirectory path to expand it]'
    )
    assert hierarchical.metadata['limited_directories'] == 1
    assert hierarchical.metadata['omitted_entries'] == 2

    truncated = render_directory_read(
        result.model_copy(update={'lines': lines[:1], 'scanned_entries': 1, 'scan_truncated': True}),
        plan_read_target(parse_read_target('.:1-1'), 3_000),
    )
    assert truncated.metadata['truncation'] == {
        'reasons': ['directory_limit'],
        'continuations': [],
    }

    deferred = plan_read_target(parse_read_target('source.txt:1-3'), 0)
    file_result = WorkspaceReadFileResult(
        path='source.txt',
        observation=None,
        lines=(),
        total_lines=3,
        complete_presentation=False,
        editable=False,
        total_bytes=5,
        observation_limit=4,
        serialization=None,
    )
    rendered_file = render_file_read(file_result, capture_source_presentation('apply_patch', 1), deferred)
    assert rendered_file.content.endswith('[Read limit reached. Continue with source.txt:1-3]')
