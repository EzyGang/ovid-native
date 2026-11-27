import asyncio
from hashlib import sha256
from pathlib import Path
from typing import cast

import pytest
from ovid_core.tools.base import ToolExecutionContext
from ovid_core.tools.models import ToolResult
from pydantic import JsonValue
from pytest_mock import MockerFixture

from ovid_native.ast.models import AstSearchRequest
from ovid_native.ast.tools import AstGrepTool
from ovid_native.fff.models import FffGrepRequest, FffMultiGrepRequest
from ovid_native.fff.tools import FffGrepTool, FffMultiGrepTool
from ovid_native.files import ReadTool, WorkspaceReadRequest
from ovid_native.files.edit_tools import tool_edit_result
from ovid_native.files.models import HashlineEditRequest
from ovid_native.files.rehydration import receipts_from_tool_results
from ovid_native.search.models import GrepToolRequest
from ovid_native.search.tools import GrepTool
from ovid_native.workspace.errors import WorkspaceObservationCollisionError
from ovid_native.workspace.evidence import WorkspaceSourcePresenter, capture_source_presentation
from ovid_native.workspace.models import WorkspaceSessionId
from ovid_native.workspace.policy import WorkspacePolicy
from ovid_native.workspace.provider_observations import ProviderWorkspaceObservationService
from ovid_native.workspace.rehydration import WorkspaceRehydrationReport, WorkspaceRehydrationRequest
from ovid_native.workspace.service import NativeWorkspaceSession


_SOURCE = 'print(alpha)\n'
_DIGEST = sha256(_SOURCE.encode()).hexdigest()


def context() -> ToolExecutionContext[None]:
    return cast('ToolExecutionContext[None]', None)


def metadata(result: ToolResult) -> JsonValue:
    return cast(JsonValue, result.metadata)


@pytest.mark.parametrize('mode', ['hashline', 'replace'])
def test_read_and_source_tools_persist_full_receipts(tmp_path: Path, mode: str) -> None:
    async def run() -> list[JsonValue]:
        (tmp_path / 'source.py').write_text(_SOURCE)
        workspace = NativeWorkspaceSession(root=tmp_path, edit_mode=mode)
        selection = workspace.edit_mode.current
        presenter = WorkspaceSourcePresenter(
            observations=workspace.observations,
            presentation=capture_source_presentation(selection.mode, selection.generation),
        )
        await workspace.fff.start()
        await workspace.fff.wait_ready(timeout_seconds=10.0)
        results = [
            await ReadTool[None](provider=workspace.files, state=workspace.edit_mode).execute(
                context(), WorkspaceReadRequest(path='source.py')
            ),
            await GrepTool[None](provider=workspace.search, presenter=presenter).execute(
                context(), GrepToolRequest(pattern='alpha')
            ),
            await FffGrepTool[None](provider=workspace.fff, presenter=presenter).execute(
                context(), FffGrepRequest(query='alpha', mode='plain')
            ),
            await FffMultiGrepTool[None](provider=workspace.fff, presenter=presenter).execute(
                context(), FffMultiGrepRequest(patterns=('alpha',))
            ),
            await AstGrepTool[None](provider=workspace.ast, presenter=presenter).execute(
                context(), AstSearchRequest(pattern='print($A)')
            ),
        ]
        await workspace.close()
        return [metadata(result) for result in results]

    for value in asyncio.run(run()):
        assert receipts_from_tool_results([value]) == (
            WorkspaceRehydrationRequest(
                path='source.py',
                content_sha256=_DIGEST,
                visible_ranges=((1, 1),),
                complete_presentation=True,
            ),
        )


def test_edit_results_persist_post_edit_receipts(tmp_path: Path) -> None:
    async def run() -> JsonValue:
        (tmp_path / 'source.txt').write_text('one\ntwo\n')
        workspace = NativeWorkspaceSession(root=tmp_path, edit_mode='hashline')
        read = await ReadTool[None](provider=workspace.files, state=workspace.edit_mode).execute(
            context(), WorkspaceReadRequest(path='source.txt; source.txt:1')
        )
        header, first = cast(str, read.content).splitlines()[:2]
        locator = first.split('|', maxsplit=1)[0]
        patch = f'*** Begin Patch\n{header}\nPUT {locator}.={locator}:\n+changed\n*** End Patch\n'
        edit = await workspace.files.hashline(HashlineEditRequest(input=patch))
        await workspace.close()
        assert len(receipts_from_tool_results([metadata(read)])) == 2
        return metadata(tool_edit_result(edit, capture_source_presentation('hashline', 1)))

    (request,) = receipts_from_tool_results([asyncio.run(run())])

    assert request.path == 'source.txt'
    assert request.content_sha256 == sha256(b'changed\ntwo\n').hexdigest()
    assert request.line_numbers == (1,)


def test_extraction_ignores_old_and_malformed_metadata() -> None:
    valid = {'path': 'a.txt', 'tag': 'ABCD', 'content_sha256': _DIGEST, 'visible_ranges': [[1, 2]]}
    values: list[JsonValue] = [
        None,
        'content',
        {'observation': 'ABCD'},
        {'observations': 'ABCD'},
        {'observations': [1, {'path': 'a.txt'}, {**valid, 'complete_presentation': False, 'visible_ranges': [[2, 1]]}]},
        {'observations': [{**valid, 'complete_presentation': True}]},
    ]

    (request,) = receipts_from_tool_results(values)

    assert request.line_numbers == (1, 2)
    assert request.complete_presentation is True


def test_provider_rehydration_verifies_digests_and_retention(tmp_path: Path, mocker: MockerFixture) -> None:
    async def run() -> tuple[WorkspaceRehydrationReport, WorkspaceRehydrationReport]:
        for index in range(257):
            (tmp_path / f'{index}.txt').write_text(f'{index}\n')
        (tmp_path / 'large.txt').write_text('x' * 128)
        workspace = NativeWorkspaceSession(root=tmp_path, policy=WorkspacePolicy(max_observation_file_bytes=64))
        service = ProviderWorkspaceObservationService(workspace.files, session_id=WorkspaceSessionId('outer'))
        requests = [_request(f'{index}.txt', f'{index}\n') for index in range(257)]
        stale = [
            _request('missing.txt', 'x'),
            _request('large.txt', 'x' * 128),
            _request('0.txt', 'changed\n'),
            _request('0.txt', '0\n', end=2),
        ]

        retained = await service.rehydrate([requests[256], *stale, requests[0], *requests[:256]])
        mocker.patch.object(service, '_record', side_effect=WorkspaceObservationCollisionError('ambiguous'))
        collided = await service.rehydrate([requests[0]])
        await workspace.close()
        return retained, collided

    retained, collided = asyncio.run(run())

    assert retained == WorkspaceRehydrationReport(restored=257, stale=4, skipped=1)
    assert collided == WorkspaceRehydrationReport(restored=0, stale=1, skipped=0)


def _request(path: str, content: str, *, end: int = 1) -> WorkspaceRehydrationRequest:
    return WorkspaceRehydrationRequest(
        path=path,
        content_sha256=sha256(content.encode()).hexdigest(),
        visible_ranges=((1, end),),
        complete_presentation=True,
    )
