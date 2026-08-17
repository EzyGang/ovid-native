from hashlib import sha256

from ovid_native.files.models import WorkspaceReadFileResult, WorkspaceTextSerialization
from ovid_native.workspace.errors import WorkspaceObservationNotFoundError, WorkspaceStaleError
from ovid_native.workspace.observations import WorkspaceObservationReceipt, WorkspaceRenderedLine


def validate_spans(
    path: str,
    spans: tuple[tuple[int, int, int, int], ...],
    claimed_lines: frozenset[int],
) -> None:
    for start_line, start_byte, end_line, end_byte in spans:
        valid_order = start_line > 0 and (start_line < end_line or (start_line == end_line and start_byte <= end_byte))
        complete_claims = all(line in claimed_lines for line in range(start_line, end_line + 1))
        if not valid_order or start_byte < 0 or end_byte < 0 or not complete_claims:
            raise WorkspaceObservationNotFoundError(
                f'Source evidence contains an invalid span: {path}:{start_line}-{end_line}'
            )


def validate_complete_source(
    path: str,
    result: WorkspaceReadFileResult,
    receipt: WorkspaceObservationReceipt,
) -> WorkspaceTextSerialization:
    expected_lines = tuple(range(1, result.total_lines + 1))
    actual_lines = tuple(line.line_number for line in result.lines)
    serialization = result.serialization
    if not result.complete_presentation or actual_lines != expected_lines or serialization is None:
        raise WorkspaceObservationNotFoundError(f'Complete source evidence is unavailable: {path}')

    normalized = _normalized_source(result.lines, serialization)
    if _digest(normalized) != receipt.content_sha256:
        raise WorkspaceStaleError(f'Complete source identity changed before presentation: {path}')
    return serialization


def validate_serialized_spans(
    path: str,
    spans: tuple[tuple[int, int, int, int], ...],
    lines: dict[int, WorkspaceRenderedLine],
    serialization: WorkspaceTextSerialization,
) -> None:

    ordered_lines = tuple(lines[number] for number in sorted(lines))
    source = _serialized_source(_normalized_source(ordered_lines, serialization), serialization)
    bounds = _serialized_line_bounds(lines, serialization)
    for start_line, start_byte, end_line, end_byte in spans:
        start = bounds[start_line]
        end = bounds[end_line]
        end_byte = _terminal_span_end(end_byte, end, serialization)
        valid_boundaries = (
            start[0] <= start_byte <= start[1]
            and end[0] <= end_byte <= end[1]
            and _is_utf8_boundary(source, start_byte)
            and _is_utf8_boundary(source, end_byte)
        )
        if not valid_boundaries:
            raise WorkspaceObservationNotFoundError(
                f'Source evidence contains an invalid UTF-8 span: {path}:{start_line}-{end_line}'
            )


def _terminal_span_end(
    end_byte: int,
    bounds: tuple[int, int],
    serialization: WorkspaceTextSerialization,
) -> int:
    ending_bytes = len({'lf': b'\n', 'crlf': b'\r\n', 'cr': b'\r'}[serialization.line_ending])
    if bounds[1] < end_byte <= bounds[1] + ending_bytes:
        return bounds[1]
    return end_byte


def _digest(text: str) -> str:
    return sha256(text.encode()).hexdigest()


def _normalized_source(
    lines: tuple[WorkspaceRenderedLine, ...],
    serialization: WorkspaceTextSerialization,
) -> str:
    source = '\n'.join(line.text for line in lines)
    return f'{source}\n' if serialization.terminal_newline else source


def _serialized_source(source: str, serialization: WorkspaceTextSerialization) -> bytes:
    ending = {'lf': '\n', 'crlf': '\r\n', 'cr': '\r'}[serialization.line_ending]
    encoded = source.replace('\n', ending).encode()
    return b'\xef\xbb\xbf' + encoded if serialization.bom else encoded


def _serialized_line_bounds(
    lines: dict[int, WorkspaceRenderedLine],
    serialization: WorkspaceTextSerialization,
) -> dict[int, tuple[int, int]]:
    offset = 3 if serialization.bom else 0
    ending_bytes = len({'lf': b'\n', 'crlf': b'\r\n', 'cr': b'\r'}[serialization.line_ending])
    bounds: dict[int, tuple[int, int]] = {}
    for line_number in sorted(lines):
        end = offset + len(lines[line_number].text.encode())
        bounds[line_number] = (offset, end)
        offset = end + ending_bytes
    return bounds


def _is_utf8_boundary(source: bytes, index: int) -> bool:
    try:
        source[:index].decode()
    except UnicodeDecodeError:
        return False
    return True
