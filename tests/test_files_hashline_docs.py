import asyncio
import re
from pathlib import Path

import pytest

from ovid_native.files.edit_tools import HashlineEditTool
from ovid_native.files.hashline import HASHLINE_EXAMPLES, HASHLINE_GRAMMAR, parse_hashline
from ovid_native.files.models import HashlineEditRequest, WorkspaceFileReadRequest
from ovid_native.workspace.errors import WorkspacePatchError
from ovid_native.workspace.service import NativeWorkspaceSession


def numbered(count: int) -> str:
    return ''.join(f'value_{line} = {line}\n' for line in range(1, count + 1))


async def bind_example(workspace: NativeWorkspaceSession, example: str) -> str:
    path = re.search(r'^\[(.+)#[0-9A-F]{4}\]$', example, flags=re.MULTILINE)
    assert path is not None
    observed = await workspace.files.read_file(WorkspaceFileReadRequest(path=path.group(1)))
    assert observed.observation is not None
    hashes = {line.line_number: line.short_hash for line in observed.lines}
    tagged = re.sub(r'#[0-9A-F]{4}\]', f'#{observed.observation.tag}]', example)
    return re.sub(r'\b(\d+):[0-9A-F]{2}\b', lambda match: f'{match[1]}:{hashes[int(match[1])]}', tagged)


def test_hashline_examples_parse() -> None:
    paths = [parse_hashline(example)[0][0] for example in HASHLINE_EXAMPLES]

    assert paths == ['src/app.py', 'src/util.py']


def test_hashline_examples_apply_through_installed_extension(tmp_path: Path) -> None:
    async def run() -> None:
        (tmp_path / 'src').mkdir()
        app = tmp_path / 'src' / 'app.py'
        util = tmp_path / 'src' / 'util.py'
        app.write_text(numbered(20))
        util.write_text(numbered(30))
        workspace = NativeWorkspaceSession(root=tmp_path, edit_mode='hashline')

        for example in HASHLINE_EXAMPLES:
            await workspace.files.hashline(HashlineEditRequest(input=await bind_example(workspace, example)))
        await workspace.close()

        app_lines = numbered(20).splitlines(keepends=True)
        util_lines = numbered(30).splitlines(keepends=True)
        expected_app = [*app_lines[:11], '    return compute(value)\n', *app_lines[13:], "    log.debug('done')\n"]
        expected_util = [*util_lines[:4], *util_lines[8:29], *util_lines[4:8], util_lines[29]]
        assert app.read_text() == ''.join(expected_app)
        assert not util.exists()
        assert (tmp_path / 'src' / 'helpers.py').read_text() == ''.join(expected_util)

    asyncio.run(run())


def test_hashline_description_names_every_statement_and_locator() -> None:
    description = HashlineEditTool.description
    rules = '\n'.join(line for line in HASHLINE_GRAMMAR.splitlines() if re.match(r'^\??[a-z_]+:', line))
    keywords = set(re.findall(r'"([^"]+)"', rules))
    required = {'PUT', 'CUT', 'REM', 'MV', '.=', '*', '<', '>', '<^', '>$', '@', '+', '*** Begin Patch'}

    assert keywords >= {'PUT', 'CUT', 'REM', 'MV', '<^', '>$', '*** End Patch'}
    assert all(keyword in description for keyword in keywords | required)
    assert all(example in description for example in HASHLINE_EXAMPLES)
    assert len(description) <= 2500


def test_hashline_parse_errors_name_expected_tokens_and_offending_line() -> None:
    with pytest.raises(WorkspacePatchError) as locator:
        parse_hashline('*** Begin Patch\n[a.py#ABCD]\nPUT foo:\n+x\n*** End Patch\n')
    with pytest.raises(WorkspacePatchError) as newline:
        parse_hashline('*** Begin Patch\r')
    with pytest.raises(WorkspacePatchError) as long_line:
        parse_hashline(f'*** Begin Patch\n[a.py#ABCD]\nPUT {"x" * 200}\n*** End Patch\n')

    assert str(locator.value) == (
        'Malformed Hashline input at line 3, column 5: expected one of <, <^, >, >$, LINE:hh. Line 3: PUT foo:'
    )
    assert str(newline.value) == (
        'Malformed Hashline input at line 1, column 16: expected one of newline. Line 1: *** Begin Patch'
    )
    assert str(long_line.value).endswith(f'Line 3: PUT {"x" * 116}')
