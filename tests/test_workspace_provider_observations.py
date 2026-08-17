import asyncio
from hashlib import sha256

import pytest
from pytest_mock import MockerFixture

from ovid_native.files.models import WorkspaceFileReadRequest, WorkspaceReadFileResult, WorkspaceTextSerialization
from ovid_native.workspace.errors import (
    WorkspaceObservationCollisionError,
    WorkspaceObservationNotFoundError,
    WorkspaceObservedLineChangedError,
    WorkspaceStaleError,
    WorkspaceUnseenLineError,
)
from ovid_native.workspace.models import WorkspaceSessionId
from ovid_native.workspace.observations import (
    WorkspaceLineRange,
    WorkspaceLineValidationRequest,
    WorkspaceObservationReceipt,
    WorkspaceObservationRequest,
    WorkspaceRenderedLine,
)
from ovid_native.workspace.provider_observations import ProviderWorkspaceObservationService


_LF_SERIALIZATION = WorkspaceTextSerialization(bom=False, line_ending='lf', terminal_newline=True)


_ALPHA_DIGEST = sha256(b'alpha\n').hexdigest()


def test_provider_observations_retain_full_line_digests(mocker: MockerFixture) -> None:
    async def run() -> None:
        files = mocker.Mock()
        files.read_file = mocker.AsyncMock(return_value=_read_result())
        service = ProviderWorkspaceObservationService(files, session_id=WorkspaceSessionId('outer'))
        assert service.session_id == WorkspaceSessionId('outer')
        observed = await service.observe_claims(
            path='source.py',
            claims=((1, 'alpha'),),
            spans=((1, 0, 1, 5),),
            complete_presentation=False,
        )

        assert observed.observation is not None
        assert observed.observation.session_id == WorkspaceSessionId('outer')
        assert (await service.resolve_observation('source.py', 'abcd')) == observed.observation

        files.read_file.return_value = _read_result(receipt=_receipt(digest='b' * 64))
        validated = await service.validate_observed_lines(
            WorkspaceLineValidationRequest(path='source.py', tag='abcd', line_numbers=(1,))
        )
        assert validated.observation == observed.observation

        files.read_file.return_value = _read_result(
            receipt=_receipt(digest='c' * 64),
            lines=(WorkspaceRenderedLine(line_number=1, short_hash='11', text='changed'),),
        )
        with pytest.raises(WorkspaceObservedLineChangedError, match='source.py:1'):
            await service.validate_observed_lines(
                WorkspaceLineValidationRequest(path='source.py', tag='abcd', line_numbers=(1,))
            )

        files.read_file.return_value = _read_result()
        with pytest.raises(WorkspaceUnseenLineError, match='source.py:2'):
            await service.validate_observed_lines(
                WorkspaceLineValidationRequest(path='source.py', tag='abcd', line_numbers=(1, 2))
            )

    asyncio.run(run())


def test_provider_observations_reject_invalid_or_stale_evidence(mocker: MockerFixture) -> None:
    async def run() -> None:
        files = mocker.Mock()
        files.read_file = mocker.AsyncMock(return_value=_read_result())
        service = ProviderWorkspaceObservationService(files, session_id=WorkspaceSessionId('outer'))

        with pytest.raises(WorkspaceStaleError, match='cannot validate revision'):
            await service.observe_file(
                WorkspaceObservationRequest(path='source.py', expected_revision='old', visible_ranges=())
            )
        with pytest.raises(WorkspaceObservationNotFoundError, match='duplicate lines'):
            await service.observe_claims(
                path='source.py',
                claims=((1, 'alpha'), (1, 'alpha')),
                spans=(),
                complete_presentation=False,
            )
        with pytest.raises(WorkspaceObservationNotFoundError, match='invalid span'):
            await service.observe_claims(
                path='source.py',
                claims=((1, 'alpha'),),
                spans=((2, 5, 1, 0),),
                complete_presentation=False,
            )

        files.read_file.return_value = _read_result(
            lines=(WorkspaceRenderedLine(line_number=1, short_hash='11', text='changed'),)
        )
        with pytest.raises(WorkspaceStaleError, match='changed before presentation'):
            await service.observe_claims(
                path='source.py',
                claims=((1, 'alpha'),),
                spans=(),
                complete_presentation=False,
            )

        files.read_file.return_value = _read_result(complete=False)
        with pytest.raises(WorkspaceObservationNotFoundError, match='Complete source evidence is unavailable'):
            await service.observe_claims(
                path='source.py',
                claims=((1, 'alpha'),),
                spans=(),
                complete_presentation=True,
            )
        for incomplete in (_read_result(total_lines=2), _read_result(serialization=None)):
            files.read_file.return_value = incomplete
            with pytest.raises(WorkspaceObservationNotFoundError, match='Complete source evidence is unavailable'):
                await service.observe_claims(
                    path='source.py',
                    claims=((1, 'alpha'),),
                    spans=(),
                    complete_presentation=False,
                )

        files.read_file.return_value = _read_result(receipt=_receipt(digest='d' * 64))
        with pytest.raises(WorkspaceStaleError, match='Complete source identity changed'):
            await service.observe_claims(
                path='source.py',
                claims=((1, 'alpha'),),
                spans=(),
                complete_presentation=False,
            )

        utf_line = WorkspaceRenderedLine(line_number=1, short_hash='22', text='é')
        utf_receipt = _receipt(digest=sha256('é'.encode()).hexdigest())
        files.read_file.return_value = _read_result(
            receipt=utf_receipt,
            lines=(utf_line,),
            total_bytes=5,
            serialization=WorkspaceTextSerialization(bom=True, line_ending='lf', terminal_newline=False),
        )
        with pytest.raises(WorkspaceObservationNotFoundError, match='invalid UTF-8 span'):
            await service.observe_claims(
                path='source.py',
                claims=((1, 'é'),),
                spans=((1, 4, 1, 5),),
                complete_presentation=False,
            )

        files.read_file.return_value = _read_result(receipt=None, editable=False)
        uneditable = await service.observe_file(WorkspaceObservationRequest(path='source.py', visible_ranges=()))
        assert uneditable.observation is None
        assert uneditable.editable is False
        with pytest.raises(WorkspaceObservationNotFoundError, match='returned no observation'):
            await service.observe_claims(
                path='source.py',
                claims=((1, 'alpha'),),
                spans=(),
                complete_presentation=False,
            )
        with pytest.raises(WorkspaceObservationNotFoundError, match='was not retained'):
            await service.resolve_observation('source.py', 'ffff')

    asyncio.run(run())


def test_provider_observation_collisions_become_tombstones(mocker: MockerFixture) -> None:
    async def run() -> None:
        files = mocker.Mock()
        files.read_file = mocker.AsyncMock(return_value=_read_result())
        service = ProviderWorkspaceObservationService(files, session_id=WorkspaceSessionId('outer'))
        request = WorkspaceObservationRequest(path='source.py', visible_ranges=())

        await service.observe_file(request)
        files.read_file.return_value = _read_result(receipt=_receipt(digest='b' * 64))
        with pytest.raises(WorkspaceObservationCollisionError, match='ambiguous'):
            await service.observe_file(request)
        with pytest.raises(WorkspaceObservationCollisionError, match='ambiguous'):
            await service.resolve_observation('source.py', 'abcd')
        with pytest.raises(WorkspaceObservationCollisionError, match='ambiguous'):
            await service.observe_file(request)

    asyncio.run(run())


def test_provider_observation_retention_is_bounded(mocker: MockerFixture) -> None:
    async def read_file(request: WorkspaceFileReadRequest) -> WorkspaceReadFileResult:
        path = request.path
        tag = f'{int(path.removesuffix(".py")):04X}'
        return _read_result(path=path, receipt=_receipt(path=path, tag=tag))

    async def run() -> None:
        files = mocker.Mock()
        files.read_file = mocker.AsyncMock(side_effect=read_file)
        service = ProviderWorkspaceObservationService(files, session_id=WorkspaceSessionId('outer'))

        for index in range(257):
            await service.observe_file(
                WorkspaceObservationRequest(
                    path=f'{index}.py',
                    visible_ranges=(WorkspaceLineRange(start=1, end=1), WorkspaceLineRange(start=3, end=3)),
                )
            )

        with pytest.raises(WorkspaceObservationNotFoundError, match='was not retained'):
            await service.resolve_observation('0.py', '0000')
        assert (await service.resolve_observation('256.py', '0100')).path == '256.py'

    asyncio.run(run())


def _receipt(
    *,
    path: str = 'source.py',
    tag: str = 'ABCD',
    digest: str = _ALPHA_DIGEST,
) -> WorkspaceObservationReceipt:
    return WorkspaceObservationReceipt(
        session_id=WorkspaceSessionId('inner'),
        path=path,
        tag=tag,
        content_sha256=digest,
        generation=1,
        visible_ranges=(WorkspaceLineRange(start=1, end=1),),
        complete_presentation=True,
    )


def _read_result(
    *,
    path: str = 'source.py',
    receipt: WorkspaceObservationReceipt | None = _receipt(),
    lines: tuple[WorkspaceRenderedLine, ...] = (WorkspaceRenderedLine(line_number=1, short_hash='11', text='alpha'),),
    complete: bool = True,
    editable: bool = True,
    total_bytes: int = 6,
    serialization: WorkspaceTextSerialization | None = _LF_SERIALIZATION,
    total_lines: int = 1,
) -> WorkspaceReadFileResult:
    return WorkspaceReadFileResult(
        path=path,
        observation=receipt,
        lines=lines,
        total_lines=total_lines,
        complete_presentation=complete,
        editable=editable,
        total_bytes=total_bytes,
        observation_limit=1024,
        serialization=serialization,
    )
