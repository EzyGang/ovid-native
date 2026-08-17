from collections.abc import Collection
from typing import Literal, Self

from ovid_core.models import BaseModel
from pydantic import Field, model_validator

from ovid_native.workspace.errors import WorkspaceError, WorkspaceStaleError
from ovid_native.workspace.models import WorkspaceSessionId
from ovid_native.workspace.observations import (
    WorkspaceLineRange,
    WorkspaceObservationReceipt,
    WorkspaceObservationService,
    WorkspaceRenderedLine,
)


class WorkspaceSourcePresentation(BaseModel):
    mode: str
    mode_generation: int = Field(ge=1)
    format: Literal['plain', 'hashline']


class WorkspaceSourceLineClaim(BaseModel):
    line_number: int = Field(ge=1)
    text: str


class WorkspaceSourceSpanClaim(BaseModel):
    start_line: int = Field(ge=1)
    start_byte: int = Field(ge=0)
    end_line: int = Field(ge=1)
    end_byte: int = Field(ge=0)

    @model_validator(mode='after')
    def validate_order(self) -> Self:
        if (self.end_line, self.end_byte) < (self.start_line, self.start_byte):
            raise ValueError('source span end must not precede its start')
        return self


class WorkspaceEvidence(BaseModel):
    path: str = Field(min_length=1)
    revision: str | None = None
    lines: tuple[WorkspaceSourceLineClaim, ...]
    visible_ranges: tuple[WorkspaceLineRange, ...]
    spans: tuple[WorkspaceSourceSpanClaim, ...] = ()
    complete: bool = False


class WorkspaceTrustedSourceReceipt(BaseModel):
    session_id: WorkspaceSessionId
    provider_generation: int = Field(ge=1)
    file_generation: int = Field(ge=1)
    evidence: WorkspaceEvidence
    complete_source_sha256: str = Field(pattern=r'^[0-9a-f]{64}$')


class WorkspaceObservationRequest(BaseModel):
    evidence: WorkspaceEvidence
    trusted_receipt: WorkspaceTrustedSourceReceipt | None = None
    expected_revision: str | None = None
    purpose: Literal['read', 'grep', 'fff_grep', 'ast_grep']
    presentation: WorkspaceSourcePresentation


class EditableSourceGroup(BaseModel):
    path: str
    observation: WorkspaceObservationReceipt | None
    editable: bool
    lines: tuple[WorkspaceRenderedLine, ...]
    visible_ranges: tuple[WorkspaceLineRange, ...]
    uneditable_reason: str | None = None

    def render(self, presentation: WorkspaceSourcePresentation) -> str:
        if presentation.format == 'hashline' and self.observation is not None and self.editable:
            rows = [f'[{self.path}#{self.observation.tag}]']
            rows.extend(f'{line.line_number}:{line.short_hash}|{line.text}' for line in self.lines)
            return '\n'.join(rows)

        rows = [f'[{self.path}]']
        rows.extend(f'{line.line_number}:{line.text}' for line in self.lines)
        if self.uneditable_reason is not None:
            rows.append(f'[uneditable: {self.uneditable_reason}]')
        return '\n'.join(rows)


class WorkspaceSourcePresenter:
    def __init__(
        self,
        *,
        observations: WorkspaceObservationService,
        presentation: WorkspaceSourcePresentation,
        provider_generation: int = 1,
    ) -> None:
        self._observations = observations
        self.presentation = presentation
        self._provider_generation = provider_generation

    async def observe(self, request: WorkspaceObservationRequest) -> EditableSourceGroup:
        evidence = request.evidence
        stale_reason = self._validate_request(request)
        if stale_reason is not None:
            return _uneditable(evidence, stale_reason)

        try:
            observed = await self._observations.observe_claims(
                path=evidence.path,
                claims=tuple((line.line_number, line.text) for line in evidence.lines),
                spans=tuple(
                    (span.start_line, span.start_byte, span.end_line, span.end_byte) for span in evidence.spans
                ),
                complete_presentation=evidence.complete,
            )
            self._validate_trusted(request, observed.observation)
        except WorkspaceError as error:
            return _uneditable(evidence, str(error))

        return EditableSourceGroup(
            path=observed.path,
            observation=observed.observation,
            editable=observed.editable,
            lines=observed.lines,
            visible_ranges=evidence.visible_ranges,
        )

    def _validate_request(self, request: WorkspaceObservationRequest) -> str | None:
        evidence = request.evidence
        if request.expected_revision is not None and evidence.revision != request.expected_revision:
            return f'workspace evidence revision changed for {evidence.path}'
        trusted = request.trusted_receipt
        if trusted is not None and trusted.evidence != evidence:
            return f'trusted source evidence does not match the rendered evidence for {evidence.path}'
        return None

    def _validate_trusted(
        self,
        request: WorkspaceObservationRequest,
        observation: WorkspaceObservationReceipt | None,
    ) -> None:
        trusted = request.trusted_receipt
        if trusted is None:
            return
        if trusted.session_id != self._observations.session_id:
            raise WorkspaceStaleError(
                f'trusted source receipt belongs to another workspace session: {request.evidence.path}'
            )
        if trusted.provider_generation != self._provider_generation:
            raise WorkspaceStaleError(
                f'trusted source receipt has a stale provider generation: {request.evidence.path}'
            )
        if observation is None or trusted.complete_source_sha256 != observation.content_sha256:
            raise WorkspaceStaleError(f'trusted source receipt content changed: {request.evidence.path}')
        if trusted.file_generation != observation.generation:
            raise WorkspaceStaleError(f'trusted source receipt has a stale file generation: {request.evidence.path}')


def capture_source_presentation(mode: str, generation: int) -> WorkspaceSourcePresentation:
    return WorkspaceSourcePresentation(
        mode=mode,
        mode_generation=generation,
        format='hashline' if mode == 'hashline' else 'plain',
    )


def normalize_terminal_span_end(
    *,
    end_line: int,
    end_column: int,
    end_byte: int,
    claimed_lines: Collection[int],
) -> tuple[int, int]:
    if end_column != 1 or end_line in claimed_lines or end_line - 1 not in claimed_lines:
        return end_line, end_byte

    return end_line - 1, end_byte - 1


def _uneditable(evidence: WorkspaceEvidence, reason: str) -> EditableSourceGroup:
    lines = tuple(
        WorkspaceRenderedLine(line_number=line.line_number, short_hash='--', text=line.text) for line in evidence.lines
    )
    return EditableSourceGroup(
        path=evidence.path,
        observation=None,
        editable=False,
        lines=lines,
        visible_ranges=evidence.visible_ranges,
        uneditable_reason=reason,
    )
