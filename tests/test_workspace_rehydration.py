import asyncio
from hashlib import sha256
from pathlib import Path
from typing import cast

import pytest
from ovid_core.tools.base import ToolExecutionContext
from pydantic import JsonValue

from ovid_native.files import ReadTool, WorkspaceReadRequest
from ovid_native.files.models import HashlineEditRequest
from ovid_native.files.rehydration import receipts_from_tool_results
from ovid_native.workspace.errors import WorkspaceObservationNotFoundError, WorkspaceUnseenLineError
from ovid_native.workspace.policy import WorkspacePolicy
from ovid_native.workspace.rehydration import WorkspaceRehydrationReport
from ovid_native.workspace.service import NativeWorkspaceSession


def context() -> ToolExecutionContext[None]:
    return cast('ToolExecutionContext[None]', None)


def replace_line(header: str, rendered: str, text: str) -> str:
    locator = rendered.split('|', maxsplit=1)[0]
    return f'*** Begin Patch\n{header}\nPUT {locator}.={locator}:\n+{text}\n*** End Patch\n'


async def read_tool_result(
    root: Path,
    path: str,
    policy: WorkspacePolicy | None = None,
) -> tuple[str, dict[str, JsonValue]]:
    workspace = NativeWorkspaceSession(root=root, edit_mode='hashline', policy=policy)
    result = await ReadTool[None](provider=workspace.files, state=workspace.edit_mode).execute(
        context(), WorkspaceReadRequest(path=path)
    )
    await workspace.close()
    return cast(str, result.content), cast('dict[str, JsonValue]', result.metadata)


def test_rehydrated_read_authorizes_the_old_anchor_after_restart(tmp_path: Path) -> None:
    async def run() -> None:
        source = tmp_path / 'source.txt'
        source.write_text('one\ntwo\nthree\n')
        content, metadata = await read_tool_result(tmp_path, 'source.txt')
        header, _, second, _ = content.splitlines()
        workspace = NativeWorkspaceSession(root=tmp_path, edit_mode='hashline')

        with pytest.raises(WorkspaceObservationNotFoundError, match='not retained'):
            await workspace.files.hashline(HashlineEditRequest(input=replace_line(header, second, 'changed')))
        report = await workspace.observations.rehydrate(receipts_from_tool_results([metadata]))
        await workspace.files.hashline(HashlineEditRequest(input=replace_line(header, second, 'changed')))

        assert report == WorkspaceRehydrationReport(restored=1, stale=0, skipped=0)
        assert source.read_text() == 'one\nchanged\nthree\n'
        await workspace.close()

    asyncio.run(run())


def test_changed_file_reports_stale_and_the_edit_fails_closed(tmp_path: Path) -> None:
    async def run() -> None:
        source = tmp_path / 'source.txt'
        source.write_text('one\ntwo\n')
        content, metadata = await read_tool_result(tmp_path, 'source.txt')
        header, first, _ = content.splitlines()
        source.write_text('one\ntwo\nexternal\n')
        workspace = NativeWorkspaceSession(root=tmp_path, edit_mode='hashline')

        report = await workspace.observations.rehydrate(receipts_from_tool_results([metadata]))

        assert report == WorkspaceRehydrationReport(restored=0, stale=1, skipped=0)
        with pytest.raises(WorkspaceObservationNotFoundError):
            await workspace.files.hashline(HashlineEditRequest(input=replace_line(header, first, 'rejected')))
        assert source.read_text() == 'one\ntwo\nexternal\n'
        await workspace.close()

    asyncio.run(run())


def test_rehydration_authorizes_only_rendered_lines(tmp_path: Path) -> None:
    async def run() -> None:
        source = tmp_path / 'source.txt'
        source.write_text('one\ntwo\nthree\n')
        partial, metadata = await read_tool_result(tmp_path, 'source.txt:1-2')
        complete, _ = await read_tool_result(tmp_path, 'source.txt')
        header = partial.splitlines()[0]
        third = complete.splitlines()[3]
        workspace = NativeWorkspaceSession(root=tmp_path, edit_mode='hashline')

        await workspace.observations.rehydrate(receipts_from_tool_results([metadata]))

        with pytest.raises(WorkspaceUnseenLineError, match='source.txt:3'):
            await workspace.files.hashline(HashlineEditRequest(input=replace_line(header, third, 'rejected')))
        assert source.read_text() == 'one\ntwo\nthree\n'
        await workspace.close()

    asyncio.run(run())


def test_rehydration_keeps_the_newest_receipts_within_retention(tmp_path: Path) -> None:
    async def run() -> None:
        metadata = []
        for name in ('a.txt', 'b.txt', 'c.txt'):
            (tmp_path / name).write_text(f'{name}\n')
            metadata.append((await read_tool_result(tmp_path, name))[1])
        workspace = NativeWorkspaceSession(root=tmp_path, policy=WorkspacePolicy(max_observation_entries=1))

        report = await workspace.observations.rehydrate(receipts_from_tool_results(metadata))

        assert report == WorkspaceRehydrationReport(restored=1, stale=0, skipped=2)
        tag = sha256(b'c.txt\n').hexdigest()[:4].upper()
        assert (await workspace.observations.resolve_observation('c.txt', tag)).path == 'c.txt'
        with pytest.raises(WorkspaceObservationNotFoundError):
            await workspace.observations.resolve_observation('b.txt', sha256(b'b.txt\n').hexdigest()[:4])
        await workspace.close()

    asyncio.run(run())


def test_unobserved_reads_persist_no_receipt(tmp_path: Path) -> None:
    (tmp_path / 'large.txt').write_text('large\n')

    _, metadata = asyncio.run(read_tool_result(tmp_path, 'large.txt', WorkspacePolicy(max_observation_file_bytes=2)))

    assert metadata['observations'] == []
    assert receipts_from_tool_results([metadata]) == ()
