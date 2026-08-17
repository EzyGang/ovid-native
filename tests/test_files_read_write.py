import asyncio
import hashlib
from pathlib import Path

import pytest
from pydantic import ValidationError

from ovid_native.files import (
    ReadLineRange,
    WorkspaceBinaryFileError,
    WorkspaceCreateRequest,
    WorkspaceDirectoryReadRequest,
    WorkspaceEncodingError,
    WorkspaceFileReadRequest,
    WorkspaceObservationCollisionError,
    WorkspaceObservationNotFoundError,
    WorkspaceReplaceRequest,
    WorkspaceStaleError,
    WorkspaceTextSerialization,
    WorkspaceUnseenLineError,
    WorkspaceWriteError,
)
from ovid_native.workspace.errors import WorkspacePathError
from ovid_native.workspace.observations import WorkspaceLineValidationRequest
from ovid_native.workspace.policy import WorkspacePolicy
from ovid_native.workspace.service import NativeWorkspaceSession


def test_text_reads_normalize_hash_render_and_authorize_exact_lines(tmp_path: Path) -> None:
    source = tmp_path / 'source.txt'
    source.write_bytes(b'\xef\xbb\xbfone\r\ntwo\r\nthree\r\n')
    workspace = NativeWorkspaceSession(root=tmp_path)

    result = asyncio.run(
        workspace.files.read_file(
            WorkspaceFileReadRequest(
                path='source.txt',
                ranges=(ReadLineRange(start=1, end=1), ReadLineRange(start=3)),
            )
        )
    )

    expected_sha = hashlib.sha256(b'one\ntwo\nthree\n').hexdigest()
    assert result.observation is not None
    assert result.observation.tag == expected_sha[:4].upper()
    assert result.observation.content_sha256 == expected_sha
    assert [(line_range.start, line_range.end) for line_range in result.observation.visible_ranges] == [(1, 1), (3, 3)]
    assert [line.text for line in result.lines] == ['one', 'three']
    assert all(
        len(line.short_hash) == 2
        and line.short_hash == line.short_hash.upper()
        and all(character in '0123456789ABCDEF' for character in line.short_hash)
        for line in result.lines
    )
    assert result.total_lines == 3
    assert result.complete_presentation is False
    assert result.editable is True
    assert result.serialization == WorkspaceTextSerialization(bom=True, line_ending='crlf', terminal_newline=True)
    assert result.render().startswith(f'[source.txt#{expected_sha[:4].upper()}]\n1:')
    validation = asyncio.run(
        workspace.observations.validate_observed_lines(
            WorkspaceLineValidationRequest(
                path='source.txt',
                tag=result.observation.tag.lower(),
                line_numbers=(1, 3),
            )
        )
    )
    assert validation.observation.tag == result.observation.tag
    asyncio.run(workspace.close())


def test_read_limits_ranges_and_large_file_identity(tmp_path: Path) -> None:
    source = tmp_path / 'large.txt'
    source.write_bytes(b'one\ntwo\nthree\n')
    workspace = NativeWorkspaceSession(
        root=tmp_path,
        policy=WorkspacePolicy(max_read_bytes=4, max_observation_file_bytes=1024),
    )

    partial = asyncio.run(workspace.files.read_file(WorkspaceFileReadRequest(path='large.txt')))
    assert [line.text for line in partial.lines] == ['one']
    assert partial.total_lines == 3
    assert partial.observation is not None
    assert [(line_range.start, line_range.end) for line_range in partial.observation.visible_ranges] == [(1, 1)]
    assert partial.complete_presentation is False
    assert partial.serialization == WorkspaceTextSerialization(bom=False, line_ending='lf', terminal_newline=True)

    workspace.policy.update(max_observation_file_bytes=4)
    oversized = asyncio.run(workspace.files.read_file(WorkspaceFileReadRequest(path='large.txt')))
    assert oversized.observation is None
    assert oversized.editable is False
    assert oversized.total_lines == 3
    assert oversized.serialization is None

    with pytest.raises(ValidationError, match='cannot overlap'):
        WorkspaceFileReadRequest(
            path='large.txt',
            ranges=(ReadLineRange(start=1), ReadLineRange(start=2, end=3)),
        )
    asyncio.run(workspace.close())


def test_directory_listing_is_deterministic_bounded_and_not_source(tmp_path: Path) -> None:
    (tmp_path / 'z.txt').write_text('z')
    (tmp_path / 'a').mkdir()
    (tmp_path / 'a' / 'nested.txt').write_text('nested')
    workspace = NativeWorkspaceSession(root=tmp_path)

    shallow = asyncio.run(workspace.files.list_directory(WorkspaceDirectoryReadRequest(path='.', depth=1)))
    deep = asyncio.run(workspace.files.list_directory(WorkspaceDirectoryReadRequest(path='.', depth=2)))

    assert [entry.path for entry in shallow.entries] == ['a', 'z.txt']
    assert [entry.path for entry in deep.entries] == ['a', 'a/nested.txt', 'z.txt']
    assert deep.render() == '[.]\na/\na/nested.txt\nz.txt'
    asyncio.run(workspace.close())


def test_binary_invalid_utf8_and_unsupported_paths_are_rejected(tmp_path: Path) -> None:
    (tmp_path / 'binary.bin').write_bytes(b'one\0two')
    (tmp_path / 'invalid.txt').write_bytes(b'\xff')
    workspace = NativeWorkspaceSession(root=tmp_path)

    with pytest.raises(WorkspaceBinaryFileError):
        asyncio.run(workspace.files.read_file(WorkspaceFileReadRequest(path='binary.bin')))
    with pytest.raises(WorkspaceEncodingError):
        asyncio.run(workspace.files.read_file(WorkspaceFileReadRequest(path='invalid.txt')))
    for path in ('../outside.txt', 'https://example.com/file', 'ssh://host/file', 'archive.zip:file'):
        with pytest.raises(WorkspacePathError):
            asyncio.run(workspace.files.read_file(WorkspaceFileReadRequest(path=path)))
    asyncio.run(workspace.close())


def test_create_replace_generations_events_and_parent_policy(tmp_path: Path) -> None:
    workspace = NativeWorkspaceSession(root=tmp_path)
    events = []
    subscription = workspace.change_events.subscribe(events.append)

    created = asyncio.run(workspace.files.create_file(WorkspaceCreateRequest(path='created.txt', content='one\n')))
    assert (tmp_path / 'created.txt').read_text() == 'one\n'
    assert created.changes[0].observation is not None
    assert created.changes[0].file_generation == 2
    assert created.changes[0].revision == 2
    with pytest.raises(WorkspaceWriteError):
        asyncio.run(workspace.files.create_file(WorkspaceCreateRequest(path='created.txt', content='again')))
    with pytest.raises(WorkspaceWriteError, match='policy'):
        asyncio.run(
            workspace.files.create_file(
                WorkspaceCreateRequest(path='nested/file.txt', content='nested', create_parents=True)
            )
        )

    workspace.policy.update(create_parent_directories=True)
    nested = asyncio.run(
        workspace.files.create_file(
            WorkspaceCreateRequest(path='nested/file.txt', content='nested', create_parents=True)
        )
    )
    expected = created.changes[0].observation
    assert expected is not None
    replaced = asyncio.run(
        workspace.files.replace_file(
            WorkspaceReplaceRequest(
                path='created.txt',
                content='two\r\n',
                expected_observation=expected.tag,
            )
        )
    )
    assert (tmp_path / 'created.txt').read_bytes() == b'two\r\n'
    assert replaced.changes[0].file_generation == 3
    assert [event.operation for event in events] == ['create', 'create', 'update']
    assert events[-1].revision == replaced.changes[0].revision
    assert nested.changes[0].observation is not None
    subscription.close()
    asyncio.run(workspace.close())


def test_whole_file_replace_requires_complete_current_observation(tmp_path: Path) -> None:
    source = tmp_path / 'source.txt'
    source.write_text('one\ntwo\n')
    workspace = NativeWorkspaceSession(root=tmp_path)
    partial = asyncio.run(
        workspace.files.read_file(WorkspaceFileReadRequest(path='source.txt', ranges=(ReadLineRange(start=1, end=1),)))
    )
    assert partial.observation is not None
    with pytest.raises(WorkspaceUnseenLineError):
        asyncio.run(
            workspace.files.replace_file(
                WorkspaceReplaceRequest(
                    path='source.txt',
                    content='replacement',
                    expected_observation=partial.observation.tag,
                )
            )
        )

    complete = asyncio.run(workspace.files.read_file(WorkspaceFileReadRequest(path='source.txt')))
    assert complete.observation is not None
    source.write_text('changed\n')
    with pytest.raises(WorkspaceStaleError):
        asyncio.run(
            workspace.files.replace_file(
                WorkspaceReplaceRequest(
                    path='source.txt',
                    content='replacement',
                    expected_observation=complete.observation.tag,
                )
            )
        )
    with pytest.raises(WorkspaceObservationNotFoundError):
        asyncio.run(workspace.observations.resolve_observation('source.txt', 'FFFF'))
    asyncio.run(workspace.close())


def test_observation_eviction_and_collision_tombstones(tmp_path: Path) -> None:
    source = tmp_path / 'source.txt'
    source.write_text('initial\n')
    workspace = NativeWorkspaceSession(
        root=tmp_path,
        policy=WorkspacePolicy(max_observation_entries=1),
    )
    initial = asyncio.run(workspace.files.read_file(WorkspaceFileReadRequest(path='source.txt')))
    assert initial.observation is not None
    other = tmp_path / 'other.txt'
    other.write_text('other\n')
    asyncio.run(workspace.files.read_file(WorkspaceFileReadRequest(path='other.txt')))
    with pytest.raises(WorkspaceObservationNotFoundError):
        asyncio.run(workspace.observations.resolve_observation('source.txt', initial.observation.tag))
    asyncio.run(workspace.close())

    seen: dict[str, str] = {}
    colliding: tuple[str, str] | None = None
    for index in range(100_000):
        content = f'collision-{index}\n'
        tag = hashlib.sha256(content.encode()).hexdigest()[:4]
        if previous := seen.get(tag):
            colliding = (previous, content)
            break
        seen[tag] = content
    assert colliding is not None

    first_content, second_content = colliding
    source.write_text(first_content)
    collision_workspace = NativeWorkspaceSession(
        root=tmp_path,
        policy=WorkspacePolicy(max_observation_entries=1),
    )
    first = asyncio.run(collision_workspace.files.read_file(WorkspaceFileReadRequest(path='source.txt')))
    source.write_text(second_content)
    second = asyncio.run(collision_workspace.files.read_file(WorkspaceFileReadRequest(path='source.txt')))
    assert first.observation is not None
    assert second.observation is not None
    assert first.observation.tag == second.observation.tag
    with pytest.raises(WorkspaceObservationCollisionError):
        asyncio.run(
            collision_workspace.observations.resolve_observation(
                'source.txt',
                second.observation.tag,
            )
        )
    other.write_text('evict collision\n')
    asyncio.run(collision_workspace.files.read_file(WorkspaceFileReadRequest(path='other.txt')))
    with pytest.raises(WorkspaceObservationCollisionError):
        asyncio.run(
            collision_workspace.observations.resolve_observation(
                'source.txt',
                second.observation.tag,
            )
        )
    asyncio.run(collision_workspace.close())
