import asyncio
from importlib.metadata import metadata
from pathlib import Path

import pytest
from pydantic import ValidationError
from pytest_mock import MockerFixture

from ovid_native import _native
from ovid_native.files import (
    EditMode,
    ReadLineRange,
    WorkspaceEditModeError,
    WorkspaceFileReadRequest,
    WorkspacePartialCommitError,
    WorkspaceReadError,
    WorkspaceReadRequest,
    WorkspaceStaleError,
    WorkspaceWriteRequest,
)
from ovid_native.files.models import PatchEditEntry, WorkspaceReadDirectoryResult
from ovid_native.workspace.errors import (
    WorkspaceClosedError,
    WorkspaceError,
    WorkspacePathError,
    translate_native_workspace_error,
)
from ovid_native.workspace.observations import (
    WorkspaceChangeSubscription,
    WorkspaceLineRange,
    WorkspaceObservationRequest,
)
from ovid_native.workspace.service import NativeWorkspaceSession


def test_request_models_reject_invalid_ranges_and_mutation_combinations() -> None:
    with pytest.raises(ValidationError, match='cannot precede'):
        ReadLineRange(start=2, end=1)
    with pytest.raises(ValidationError, match='cannot overlap'):
        WorkspaceFileReadRequest(
            path='source.txt',
            ranges=(ReadLineRange(start=1, end=2), ReadLineRange(start=2, end=3)),
        )
    with pytest.raises(ValidationError, match='requires expected_observation'):
        WorkspaceWriteRequest(path='source.txt', content='content', operation='replace')
    with pytest.raises(ValidationError, match='does not accept expected_observation'):
        WorkspaceWriteRequest(path='source.txt', content='content', expected_observation='ABCD')
    with pytest.raises(ValidationError, match='does not accept create_parents'):
        WorkspaceWriteRequest(
            path='source.txt',
            content='content',
            operation='replace',
            expected_observation='ABCD',
            create_parents=True,
        )
    with pytest.raises(ValidationError, match='requires diff'):
        PatchEditEntry(operation='update')
    with pytest.raises(ValidationError, match='does not accept'):
        PatchEditEntry(operation='delete', diff='@@\n-old\n+new')
    with pytest.raises(ValidationError, match='does not accept destination'):
        PatchEditEntry(operation='create', diff='+content', destination='moved.txt')

    rendered = WorkspaceReadDirectoryResult(path='.', entries=(), truncated=True).render()
    assert rendered == '[.]\n[directory listing truncated]'


def test_distribution_exposes_files_profile() -> None:
    profiles = metadata('ovid-native').get_all('Provides-Extra')
    assert profiles is not None
    assert {'files', 'all'} <= set(profiles)


def test_edit_mode_state_notifies_once_and_translates_failures(tmp_path: Path, mocker: MockerFixture) -> None:
    workspace = NativeWorkspaceSession(root=tmp_path)
    selections = []
    subscription = workspace.edit_mode.subscribe(selections.append)

    assert workspace.edit_mode.set(EditMode.APPLY_PATCH).generation == 1
    selected = workspace.edit_mode.set(EditMode.PATCH)
    assert selected.mode == 'patch'
    assert selected.generation == 2
    assert selections == [selected]
    assert workspace.edit_mode.capture().mode == 'patch'
    subscription.close()
    subscription.close()
    workspace.edit_mode.set(EditMode.REPLACE)
    assert selections == [selected]

    with pytest.raises(WorkspaceEditModeError, match='not registered'):
        workspace.edit_mode.set('unsupported')

    native_error = _native.NativeWorkspaceEditModeError('native mode failure')
    mocker.patch.object(_native, 'workspace_set_edit_mode', side_effect=native_error)
    with pytest.raises(WorkspaceEditModeError, match='native mode failure'):
        workspace.edit_mode.set(EditMode.PATCH)
    asyncio.run(workspace.close())


def test_observation_service_revision_resolution_and_closed_state(tmp_path: Path) -> None:
    (tmp_path / 'source.txt').write_text('one\ntwo\n')
    workspace = NativeWorkspaceSession(root=tmp_path)
    observed = asyncio.run(
        workspace.observations.observe_file(
            WorkspaceObservationRequest(
                path='source.txt',
                expected_revision='1',
                visible_ranges=(WorkspaceLineRange(start=1, end=1),),
            )
        )
    )
    assert observed.observation is not None
    resolved = asyncio.run(workspace.observations.resolve_observation('source.txt', observed.observation.tag))
    assert resolved == observed.observation
    without_revision = asyncio.run(
        workspace.observations.observe_file(
            WorkspaceObservationRequest(
                path='source.txt',
                visible_ranges=(WorkspaceLineRange(start=2, end=2),),
            )
        )
    )
    assert without_revision.observation is not None
    edit_mode = workspace.edit_mode
    policy = workspace.policy
    files = workspace.files
    asyncio.run(workspace.close())
    with pytest.raises(WorkspaceClosedError):
        _ = edit_mode.current
    with pytest.raises(WorkspaceClosedError):
        edit_mode.capture()
    with pytest.raises(WorkspaceClosedError):
        _ = policy.current
    with pytest.raises(WorkspaceClosedError):
        asyncio.run(files.read(WorkspaceReadRequest(path='source.txt')))


def test_observation_expected_revision_and_generic_read_dispatch(tmp_path: Path) -> None:
    (tmp_path / 'source.txt').write_text('source\n')
    workspace = NativeWorkspaceSession(root=tmp_path)

    directory = asyncio.run(workspace.files.read(WorkspaceReadRequest(path='.')))
    assert directory.kind == 'directory'
    with pytest.raises(WorkspaceReadError, match='do not accept line ranges'):
        asyncio.run(workspace.files.read(WorkspaceReadRequest(path='.', ranges=(ReadLineRange(start=1),))))
    with pytest.raises(WorkspacePathError, match='cannot inspect'):
        asyncio.run(workspace.files.read(WorkspaceReadRequest(path='missing.txt')))
    with pytest.raises(WorkspaceStaleError, match='Workspace revision changed'):
        asyncio.run(
            workspace.observations.observe_file(
                WorkspaceObservationRequest(
                    path='source.txt',
                    expected_revision='999',
                    visible_ranges=(WorkspaceLineRange(start=1, end=1),),
                )
            )
        )
    asyncio.run(workspace.close())


def test_subscriptions_and_error_translation_are_typed() -> None:
    calls: list[str] = []
    subscription = WorkspaceChangeSubscription(lambda: calls.append('closed'))
    subscription.close()
    subscription.close()
    assert calls == ['closed']

    native = _native.NativeWorkspacePartialCommitError(
        'partial',
        ['landed.txt'],
        ['pending.txt'],
    )
    translated = translate_native_workspace_error(native)
    assert isinstance(translated, WorkspacePartialCommitError)
    assert translated.landed == ('landed.txt',)
    assert translated.pending == ('pending.txt',)
    assert isinstance(translate_native_workspace_error(Exception('unknown')), WorkspaceError)
