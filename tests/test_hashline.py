import asyncio
from pathlib import Path
from typing import cast

import pytest
from ovid_core.tools.base import ToolExecutionContext

from ovid_native.ast.models import AstSearchRequest
from ovid_native.ast.tools import AstGrepTool
from ovid_native.fff.models import FffGrepRequest, FffMultiGrepRequest
from ovid_native.fff.tools import FffGrepTool, FffMultiGrepTool
from ovid_native.files.hashline import parse_hashline
from ovid_native.files.models import HashlineEditRequest, WorkspaceFileReadRequest
from ovid_native.search.models import GrepToolRequest
from ovid_native.search.tools import GrepTool
from ovid_native.workspace.errors import WorkspaceObservedLineChangedError, WorkspacePatchError
from ovid_native.workspace.evidence import WorkspaceSourcePresenter, capture_source_presentation
from ovid_native.workspace.service import NativeWorkspaceSession


def context() -> ToolExecutionContext[None]:
    return cast('ToolExecutionContext[None]', None)


def replacement(path: str, tag: str, line: int, short_hash: str, text: str) -> str:
    return f'*** Begin Patch\n[{path}#{tag}]\nPUT {line}:{short_hash}.={line}:{short_hash}:\n+{text}\n*** End Patch\n'


def patch_from_source(content: str, text: str) -> str:
    header, rendered = content.splitlines()
    locator = rendered.split('|', maxsplit=1)[0]
    return f'*** Begin Patch\n{header}\nPUT {locator}.={locator}:\n+{text}\n*** End Patch\n'


def test_read_hashline_edit_returns_fresh_follow_up_locators(tmp_path: Path) -> None:
    async def run() -> None:
        source = tmp_path / 'source.txt'
        source.write_text('one\ntwo\nthree\n')
        workspace = NativeWorkspaceSession(root=tmp_path, edit_mode='hashline')
        observed = await workspace.files.read_file(WorkspaceFileReadRequest(path='source.txt'))
        receipt = observed.observation
        assert receipt is not None

        first = await workspace.files.hashline(
            HashlineEditRequest(
                input=replacement('source.txt', receipt.tag, 2, observed.lines[1].short_hash, 'changed')
            )
        )
        assert source.read_text() == 'one\nchanged\nthree\n'
        post = first.post_edit_sources[0]
        changed = next(line for line in post.lines if line.line_number == 2)

        second = await workspace.files.hashline(
            HashlineEditRequest(
                input=replacement('source.txt', post.observation.tag, 2, changed.short_hash, 'changed again')
            )
        )
        assert source.read_text() == 'one\nchanged again\nthree\n'
        assert second.preflight_complete is True
        assert second.commit_complete is True
        await workspace.close()

    asyncio.run(run())


def test_grep_lines_authorize_hashline_without_reread(tmp_path: Path) -> None:
    async def run() -> None:
        source = tmp_path / 'source.txt'
        source.write_text('alpha\nbeta\ngamma\n')
        workspace = NativeWorkspaceSession(root=tmp_path, edit_mode='hashline')
        selection = workspace.edit_mode.current
        presenter = WorkspaceSourcePresenter(
            observations=workspace.observations,
            presentation=capture_source_presentation(selection.mode, selection.generation),
        )
        result = await GrepTool[None](provider=workspace.search, presenter=presenter).execute(
            context(),
            GrepToolRequest(pattern='beta.*gamma', multiline=True),
        )
        header, *rendered = cast(str, result.content).splitlines()
        assert [line.split('|', maxsplit=1)[1] for line in rendered] == ['beta', 'gamma']
        locator = rendered[0].split('|', maxsplit=1)[0]
        patch = f'*** Begin Patch\n{header}\nPUT {locator}.={locator}:\n+delta\n*** End Patch\n'

        await workspace.files.hashline(HashlineEditRequest(input=patch))
        assert source.read_text() == 'alpha\ndelta\ngamma\n'
        await workspace.close()

    asyncio.run(run())


def test_newline_terminated_grep_match_supplies_hashline_evidence(tmp_path: Path) -> None:
    async def run() -> None:
        source = tmp_path / 'source.txt'
        source.write_text('alpha\nbeta\n')
        workspace = NativeWorkspaceSession(root=tmp_path, edit_mode='hashline')
        selection = workspace.edit_mode.current
        presenter = WorkspaceSourcePresenter(
            observations=workspace.observations,
            presentation=capture_source_presentation(selection.mode, selection.generation),
        )
        result = await GrepTool[None](provider=workspace.search, presenter=presenter).execute(
            context(),
            GrepToolRequest(pattern='beta\\n', mode='regex', multiline=True),
        )

        assert cast(str, result.content).splitlines()[0].startswith('[source.txt#')
        await workspace.close()

    asyncio.run(run())


def test_ast_lines_authorize_hashline_without_reread(tmp_path: Path) -> None:
    async def run() -> None:
        source = tmp_path / 'source.py'
        source.write_text('value = (\n    1 +\n    2\n)\n')
        workspace = NativeWorkspaceSession(root=tmp_path, edit_mode='hashline')
        selection = workspace.edit_mode.current
        presenter = WorkspaceSourcePresenter(
            observations=workspace.observations,
            presentation=capture_source_presentation(selection.mode, selection.generation),
        )
        result = await AstGrepTool[None](provider=workspace.ast, presenter=presenter).execute(
            context(),
            AstSearchRequest(pattern='$A + $B'),
        )

        header, first, second = cast(str, result.content).splitlines()
        assert [first.split('|', 1)[1], second.split('|', 1)[1]] == ['    1 +', '    2']
        locator = first.split('|', 1)[0]
        patch = f'*** Begin Patch\n{header}\nPUT {locator}.={locator}:\n+    3 +\n*** End Patch\n'
        await workspace.files.hashline(HashlineEditRequest(input=patch))
        assert source.read_text() == 'value = (\n    3 +\n    2\n)\n'
        await workspace.close()

    asyncio.run(run())


def test_fff_exact_lines_authorize_hashline_and_stale_lines_fail(tmp_path: Path) -> None:
    async def run() -> None:
        source = tmp_path / 'source.txt'
        source.write_text('alpha\n')
        workspace = NativeWorkspaceSession(root=tmp_path, edit_mode='hashline')
        selection = workspace.edit_mode.current
        presenter = WorkspaceSourcePresenter(
            observations=workspace.observations,
            presentation=capture_source_presentation(selection.mode, selection.generation),
        )
        await workspace.fff.start()
        await workspace.fff.wait_ready(timeout_seconds=10.0)
        first = await FffGrepTool[None](provider=workspace.fff, presenter=presenter).execute(
            context(),
            FffGrepRequest(query='alpha', mode='plain'),
        )

        await workspace.files.hashline(HashlineEditRequest(input=patch_from_source(cast(str, first.content), 'beta')))
        assert source.read_text() == 'beta\n'
        second = await FffMultiGrepTool[None](provider=workspace.fff, presenter=presenter).execute(
            context(),
            FffMultiGrepRequest(patterns=('beta',)),
        )
        await workspace.files.hashline(HashlineEditRequest(input=patch_from_source(cast(str, second.content), 'gamma')))
        assert source.read_text() == 'gamma\n'
        third = await FffGrepTool[None](provider=workspace.fff, presenter=presenter).execute(
            context(),
            FffGrepRequest(query='gamma', mode='plain'),
        )
        source.write_text('external\n')

        with pytest.raises(WorkspaceObservedLineChangedError, match='source.txt:1'):
            await workspace.files.hashline(
                HashlineEditRequest(input=patch_from_source(cast(str, third.content), 'rejected'))
            )

        assert source.read_text() == 'external\n'
        await workspace.close()

    asyncio.run(run())


def test_hashline_allows_unrelated_current_change_but_rejects_changed_anchor(tmp_path: Path) -> None:
    async def run() -> None:
        source = tmp_path / 'source.txt'
        source.write_text('one\ntwo\nthree\nfour\n')
        workspace = NativeWorkspaceSession(root=tmp_path, edit_mode='hashline')
        observed = await workspace.files.read_file(WorkspaceFileReadRequest(path='source.txt'))
        receipt = observed.observation
        assert receipt is not None

        source.write_text('one\ntwo\nthree\nexternal\n')
        await workspace.files.hashline(
            HashlineEditRequest(
                input=replacement('source.txt', receipt.tag, 2, observed.lines[1].short_hash, 'changed')
            )
        )
        assert source.read_text() == 'one\nchanged\nthree\nexternal\n'

        current = await workspace.files.read_file(WorkspaceFileReadRequest(path='source.txt'))
        current_receipt = current.observation
        assert current_receipt is not None
        source.write_text('one\nstale\nthree\nexternal\n')
        with pytest.raises(WorkspaceObservedLineChangedError, match='source.txt:2'):
            await workspace.files.hashline(
                HashlineEditRequest(
                    input=replacement('source.txt', current_receipt.tag, 2, current.lines[1].short_hash, 'rejected')
                )
            )
        assert source.read_text() == 'one\nstale\nthree\nexternal\n'
        await workspace.close()

    asyncio.run(run())


def test_hashline_parser_supports_registers_moves_and_rejects_invalid_escapes() -> None:
    sections = parse_hashline(
        '*** Begin Patch\n'
        '[a.py#ABCD]\n'
        'CUT 1:01.=2:02 @body\n'
        '[b.py#1234]\n'
        'PUT >1:03 @body\n'
        'MV "moved.py"\n'
        '*** End Patch\n'
    )
    assert sections[0][2][0][0] == 'cut_range'
    assert sections[0][2][0][6] == 'body'
    assert sections[1][2][0][0] == 'put_after'
    assert sections[1][2][1][0] == 'move'
    escaped_destination = parse_hashline('*** Begin Patch\n[a.py#ABCD]\nMV moved\\]py\n*** End Patch\n')
    assert escaped_destination[0][2][0][7] == 'moved]py'

    with pytest.raises(WorkspacePatchError, match='Malformed Hashline'):
        parse_hashline('*** Begin Patch\n[a.py#ABCD]\nMV "bad\\q"\n*** End Patch\n')


def test_hashline_parser_enforces_limits_boundaries_and_unique_sections() -> None:
    operations = parse_hashline(
        '*** Begin Patch\n[source.txt#ABCD]\nPUT <^:\n+first\nPUT >$:\n+last\nREM\nMV moved.txt\n*** End Patch\n'
    )[0][2]
    assert [operation[0] for operation in operations] == ['put_begin', 'put_end', 'remove', 'move']
    assert operations[-1][7] == 'moved.txt'

    with pytest.raises(WorkspacePatchError, match='1048576-byte limit'):
        parse_hashline('x' * (1024 * 1024 + 1))
    with pytest.raises(WorkspacePatchError, match='1024-operation limit'):
        parse_hashline('*** Begin Patch\n[source.txt#ABCD]\n' + ('REM\n' * 1025) + '*** End Patch\n')
    with pytest.raises(WorkspacePatchError, match='duplicate path sections'):
        parse_hashline('*** Begin Patch\n[source.txt#ABCD]\nREM\n[source.txt#1234]\nREM\n*** End Patch\n')
    with pytest.raises(WorkspacePatchError, match='Malformed Hashline'):
        parse_hashline('*** Begin Patch\n[source.txt#ABCD]\nREM\n')
    with pytest.raises(WorkspacePatchError, match='invalid escape'):
        parse_hashline('*** Begin Patch\n[source.txt#ABCD]\nMV bad\\q\n*** End Patch\n')
