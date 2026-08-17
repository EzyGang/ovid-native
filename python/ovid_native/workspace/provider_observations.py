from dataclasses import dataclass
from hashlib import sha256

from ovid_native.files.models import ReadLineRange, WorkspaceFileReadRequest, WorkspaceReadFileResult
from ovid_native.workspace.errors import (
    WorkspaceObservationCollisionError,
    WorkspaceObservationNotFoundError,
    WorkspaceObservedLineChangedError,
    WorkspaceStaleError,
    WorkspaceUnseenLineError,
)
from ovid_native.workspace.models import WorkspaceFilesProvider, WorkspaceSessionId
from ovid_native.workspace.observations import (
    ObservedWorkspaceFile,
    WorkspaceLineRange,
    WorkspaceLineValidationRequest,
    WorkspaceLineValidationResult,
    WorkspaceObservationReceipt,
    WorkspaceObservationRequest,
    WorkspaceRenderedLine,
)
from ovid_native.workspace.source_validation import validate_complete_source, validate_serialized_spans, validate_spans


@dataclass(slots=True)
class _ProviderObservation:
    receipt: WorkspaceObservationReceipt
    line_digests: dict[int, str]


class ProviderWorkspaceObservationService:
    def __init__(self, files: WorkspaceFilesProvider, *, session_id: WorkspaceSessionId) -> None:
        self._files = files
        self._session_id = session_id
        self._entries: dict[tuple[str, str], _ProviderObservation] = {}
        self._collisions: set[tuple[str, str]] = set()

    @property
    def session_id(self) -> WorkspaceSessionId:
        return self._session_id

    async def observe_file(self, request: WorkspaceObservationRequest) -> ObservedWorkspaceFile:
        if request.expected_revision is not None:
            raise WorkspaceStaleError(f'Custom files provider cannot validate revision: {request.expected_revision}')

        result = await self._read(request.path, request.visible_ranges)
        receipt = None if result.observation is None else self._record(result.observation, result.lines)

        return ObservedWorkspaceFile(
            path=result.path,
            observation=receipt,
            lines=result.lines,
            total_lines=result.total_lines,
            complete_presentation=result.complete_presentation,
            editable=result.editable and receipt is not None,
        )

    async def observe_claims(
        self,
        *,
        path: str,
        claims: tuple[tuple[int, str], ...],
        spans: tuple[tuple[int, int, int, int], ...],
        complete_presentation: bool,
    ) -> ObservedWorkspaceFile:
        claimed = dict(claims)
        if len(claimed) != len(claims):
            raise WorkspaceObservationNotFoundError(f'Source evidence contains duplicate lines: {path}')

        validate_spans(path, spans, frozenset(claimed))
        result = await self._read(path, ())
        current = {line.line_number: line for line in result.lines}
        if any(current.get(line) is None or current[line].text != text for line, text in claims):
            raise WorkspaceStaleError(f'Source evidence changed before presentation: {path}')

        receipt = self._require_receipt(result.path, result.observation)
        serialization = validate_complete_source(path, result, receipt)
        validate_serialized_spans(path, spans, current, serialization)
        lines = tuple(current[line] for line in sorted(claimed))
        receipt = self._record(receipt, lines)

        return ObservedWorkspaceFile(
            path=result.path,
            observation=receipt,
            lines=lines,
            total_lines=result.total_lines,
            complete_presentation=complete_presentation,
            editable=result.editable,
        )

    async def resolve_observation(self, path: str, tag: str) -> WorkspaceObservationReceipt:
        return self._resolve(path, tag).receipt

    async def validate_observed_lines(
        self,
        request: WorkspaceLineValidationRequest,
    ) -> WorkspaceLineValidationResult:
        retained = self._resolve(request.path, request.tag)
        result = await self._read(request.path, _line_ranges(request.line_numbers))
        self._require_receipt(result.path, result.observation)
        current = {line.line_number: line for line in result.lines}

        for line_number in request.line_numbers:
            retained_digest = retained.line_digests.get(line_number)
            if retained_digest is None:
                raise WorkspaceUnseenLineError(f'Workspace line was not observed: {request.path}:{line_number}')

            line = current.get(line_number)
            if line is None or _digest(line.text) != retained_digest:
                raise WorkspaceObservedLineChangedError(
                    f'Observed workspace line changed: {request.path}:{line_number}'
                )

        return WorkspaceLineValidationResult(observation=retained.receipt)

    async def _read(
        self,
        path: str,
        ranges: tuple[WorkspaceLineRange, ...],
    ) -> WorkspaceReadFileResult:
        request = WorkspaceFileReadRequest(
            path=path,
            ranges=tuple(ReadLineRange(start=value.start, end=value.end) for value in ranges),
        )
        return await self._files.read_file(request)

    def _record(
        self,
        receipt: WorkspaceObservationReceipt,
        lines: tuple[WorkspaceRenderedLine, ...],
    ) -> WorkspaceObservationReceipt:
        scoped = receipt.model_copy(update={'session_id': self._session_id, 'tag': receipt.tag.upper()})
        key = (scoped.path, scoped.tag)
        if key in self._collisions:
            raise WorkspaceObservationCollisionError(
                f'Workspace observation tag is ambiguous: {scoped.path}#{scoped.tag}'
            )

        entry = self._entries.get(key)
        if entry is not None and entry.receipt.content_sha256 != scoped.content_sha256:
            self._entries.pop(key)
            self._collisions.add(key)
            raise WorkspaceObservationCollisionError(
                f'Workspace observation tag is ambiguous: {scoped.path}#{scoped.tag}'
            )

        digests = {} if entry is None else entry.line_digests
        digests.update((line.line_number, _digest(line.text)) for line in lines)
        self._entries[key] = _ProviderObservation(scoped, digests)
        if len(self._entries) > 256:
            self._entries.pop(next(iter(self._entries)))

        return scoped

    def _resolve(self, path: str, tag: str) -> _ProviderObservation:
        key = (path, tag.upper())
        if key in self._collisions:
            raise WorkspaceObservationCollisionError(f'Workspace observation tag is ambiguous: {path}#{tag.upper()}')
        try:
            return self._entries[key]
        except KeyError as error:
            raise WorkspaceObservationNotFoundError(
                f'Workspace observation was not retained: {path}#{tag.upper()}'
            ) from error

    @staticmethod
    def _require_receipt(
        path: str,
        receipt: WorkspaceObservationReceipt | None,
    ) -> WorkspaceObservationReceipt:
        if receipt is None:
            raise WorkspaceObservationNotFoundError(f'Custom files provider returned no observation: {path}')
        return receipt


def _digest(text: str) -> str:
    return sha256(text.encode()).hexdigest()


def _line_ranges(lines: tuple[int, ...]) -> tuple[WorkspaceLineRange, ...]:
    ranges: list[WorkspaceLineRange] = []
    for line in sorted(set(lines)):
        if ranges and line == ranges[-1].end + 1:
            ranges[-1] = WorkspaceLineRange(start=ranges[-1].start, end=line)
        else:
            ranges.append(WorkspaceLineRange(start=line, end=line))
    return tuple(ranges)
