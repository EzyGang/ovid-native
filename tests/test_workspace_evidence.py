import asyncio
from pathlib import Path

import pytest
from pydantic import ValidationError
from pytest_mock import MockerFixture

from ovid_native.files.models import WorkspaceFileReadRequest, WorkspaceTextSerialization
from ovid_native.workspace.evidence import (
    WorkspaceEvidence,
    WorkspaceObservationRequest,
    WorkspaceSourceLineClaim,
    WorkspaceSourcePresenter,
    WorkspaceSourceSpanClaim,
    WorkspaceTrustedSourceReceipt,
    capture_source_presentation,
    normalize_terminal_span_end,
)
from ovid_native.workspace.models import WorkspaceSessionId
from ovid_native.workspace.observations import ObservedWorkspaceFile, WorkspaceLineRange, WorkspaceRenderedLine
from ovid_native.workspace.service import NativeWorkspaceSession
from ovid_native.workspace.source_validation import validate_serialized_spans


def test_terminal_span_end_normalizes_an_unclaimed_synthetic_line() -> None:
    assert normalize_terminal_span_end(
        end_line=3,
        end_column=1,
        end_byte=11,
        claimed_lines={2},
    ) == (2, 11)
    assert normalize_terminal_span_end(
        end_line=2,
        end_column=1,
        end_byte=10,
        claimed_lines={2},
    ) == (2, 10)


def test_crlf_terminal_span_uses_the_preceding_serialized_line_end() -> None:
    validate_serialized_spans(
        'source.txt',
        ((2, 7, 2, 13),),
        {
            1: WorkspaceRenderedLine(line_number=1, short_hash='--', text='alpha'),
            2: WorkspaceRenderedLine(line_number=2, short_hash='--', text='beta'),
        },
        WorkspaceTextSerialization(bom=False, line_ending='crlf', terminal_newline=True),
    )


def test_evidence_ranges_and_stale_revisions_render_uneditable_source(tmp_path: Path) -> None:
    with pytest.raises(ValidationError, match='must not precede'):
        WorkspaceSourceSpanClaim(start_line=2, start_byte=2, end_line=1, end_byte=1)

    async def run() -> None:
        (tmp_path / 'source.txt').write_text('one\n')
        workspace = NativeWorkspaceSession(root=tmp_path, edit_mode='hashline')
        evidence = WorkspaceEvidence(
            path='source.txt',
            revision='old',
            lines=(WorkspaceSourceLineClaim(line_number=1, text='one'),),
            visible_ranges=(WorkspaceLineRange(start=1, end=1),),
        )
        presenter = WorkspaceSourcePresenter(
            observations=workspace.observations,
            presentation=capture_source_presentation('hashline', 1),
        )
        group = await presenter.observe(
            WorkspaceObservationRequest(
                evidence=evidence,
                expected_revision='new',
                purpose='grep',
                presentation=presenter.presentation,
            )
        )

        assert group.editable is False
        assert group.render(presenter.presentation).splitlines() == [
            '[source.txt]',
            '1:one',
            '[uneditable: workspace evidence revision changed for source.txt]',
        ]
        invalid_span = evidence.model_copy(
            update={
                'revision': None,
                'spans': (WorkspaceSourceSpanClaim(start_line=1, start_byte=1, end_line=1, end_byte=99),),
            }
        )
        invalid = await presenter.observe(
            WorkspaceObservationRequest(
                evidence=invalid_span,
                purpose='grep',
                presentation=presenter.presentation,
            )
        )
        assert invalid.uneditable_reason == 'source evidence has an invalid UTF-8 span: source.txt:1'
        await workspace.close()

    asyncio.run(run())


def test_trusted_source_receipts_validate_every_identity_dimension(
    tmp_path: Path,
    mocker: MockerFixture,
) -> None:
    async def run() -> None:
        (tmp_path / 'source.txt').write_text('one\n')
        workspace = NativeWorkspaceSession(root=tmp_path, edit_mode='hashline')
        current = await workspace.files.read_file(WorkspaceFileReadRequest(path='source.txt'))
        assert current.observation is not None
        evidence = WorkspaceEvidence(
            path='source.txt',
            revision=None,
            lines=(WorkspaceSourceLineClaim(line_number=1, text='one'),),
            visible_ranges=(WorkspaceLineRange(start=1, end=1),),
        )
        trusted = WorkspaceTrustedSourceReceipt(
            session_id=workspace.id,
            provider_generation=1,
            file_generation=current.observation.generation,
            evidence=evidence,
            complete_source_sha256=current.observation.content_sha256,
        )
        presenter = WorkspaceSourcePresenter(
            observations=workspace.observations,
            presentation=capture_source_presentation('hashline', 1),
        )

        mismatched_evidence = evidence.model_copy(update={'revision': 'different'})
        mismatch = await presenter.observe(
            WorkspaceObservationRequest(
                evidence=mismatched_evidence,
                trusted_receipt=trusted,
                purpose='ast_grep',
                presentation=presenter.presentation,
            )
        )
        assert (
            mismatch.uneditable_reason == 'trusted source evidence does not match the rendered evidence for source.txt'
        )

        variants = (
            trusted.model_copy(update={'session_id': WorkspaceSessionId('another-session-identity')}),
            trusted.model_copy(update={'provider_generation': 2}),
            trusted.model_copy(update={'complete_source_sha256': '0' * 64}),
            trusted.model_copy(update={'file_generation': trusted.file_generation + 1}),
        )
        for variant in variants:
            group = await presenter.observe(
                WorkspaceObservationRequest(
                    evidence=evidence,
                    trusted_receipt=variant,
                    purpose='ast_grep',
                    presentation=presenter.presentation,
                )
            )
            assert group.editable is False
            assert group.observation is None

        accepted = await presenter.observe(
            WorkspaceObservationRequest(
                evidence=evidence,
                trusted_receipt=trusted,
                purpose='ast_grep',
                presentation=presenter.presentation,
            )
        )
        assert accepted.editable is True
        assert accepted.observation is not None
        assert accepted.render(capture_source_presentation('apply_patch', 1)) == '[source.txt]\n1:one'

        no_receipt = mocker.Mock()
        no_receipt.session_id = workspace.id
        no_receipt.observe_claims = mocker.AsyncMock(
            return_value=ObservedWorkspaceFile(
                path='source.txt',
                observation=None,
                lines=(),
                total_lines=1,
                complete_presentation=False,
                editable=False,
            )
        )
        missing_presenter = WorkspaceSourcePresenter(
            observations=no_receipt,
            presentation=capture_source_presentation('hashline', 1),
        )
        missing = await missing_presenter.observe(
            WorkspaceObservationRequest(
                evidence=evidence,
                trusted_receipt=trusted,
                purpose='ast_grep',
                presentation=missing_presenter.presentation,
            )
        )
        assert missing.uneditable_reason == 'trusted source receipt content changed: source.txt'
        await workspace.close()

    asyncio.run(run())
