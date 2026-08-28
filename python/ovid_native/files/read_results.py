from dataclasses import dataclass

from ovid_native.files.models import (
    ReadLineRange,
    WorkspaceDirectoryTreeLine,
    WorkspaceReadDirectoryTreeResult,
    WorkspaceReadFileResult,
)
from ovid_native.files.read_targets import PlannedReadTarget, format_selector
from ovid_native.files.tool_metadata import (
    WorkspaceDirectoryReadToolMetadata,
    WorkspaceFileReadToolMetadata,
    WorkspaceReadErrorToolMetadata,
    WorkspaceReadTargetToolMetadata,
    WorkspaceReadTruncationReason,
)
from ovid_native.workspace.evidence import WorkspaceSourcePresentation
from ovid_native.workspace.observations import WorkspaceRenderedLine


@dataclass(frozen=True, slots=True)
class RenderedReadTarget:
    content: str
    metadata: WorkspaceReadTargetToolMetadata


def render_file_read(
    result: WorkspaceReadFileResult,
    presentation: WorkspaceSourcePresentation,
    target: PlannedReadTarget,
) -> RenderedReadTarget:
    hashline = presentation.format == 'hashline' and result.observation is not None and result.editable
    header = f'[{result.path}#{result.observation.tag}]' if hashline else f'[{result.path}]'
    rows = [header, *_render_source_lines(result.lines, hashline)]
    visible_ranges = _line_ranges(tuple(line.line_number for line in result.lines))
    expected = _expected_lines(target.ranges, result.total_lines)
    returned = {line.line_number for line in result.lines}
    missing = tuple(number for number in expected if number not in returned)
    remaining = _remaining_ranges(target, result.total_lines)
    unmatched = tuple(line_range for line_range in target.requested_ranges if line_range.start > result.total_lines)
    reasons: list[WorkspaceReadTruncationReason] = []
    continuations: list[str] = []

    if missing:
        reasons.append('byte_limit')
        if result.lines:
            continuations.append(format_selector(result.path, _line_ranges(missing)))
        else:
            rows.append(f'[Line {missing[0]} exceeds the read byte limit and was not rendered]')
    if remaining:
        reasons.append('line_limit')
        continuations.append(format_selector(result.path, remaining))
    if result.total_lines == 0:
        rows.append('[empty file]')
    elif unmatched:
        rendered = ','.join(_format_range(line_range) for line_range in unmatched)
        rows.append(f'[No lines found for {rendered}; file has {result.total_lines} lines]')
    if continuations:
        rows.append(f'[Read limit reached. Continue with {continuations[0]}]')

    metadata: WorkspaceFileReadToolMetadata = {
        'status': 'ok',
        'kind': 'file',
        'requested': target.argument,
        'path': result.path,
        'total_bytes': result.total_bytes,
        'total_lines': result.total_lines,
        'returned_lines': len(result.lines),
        'visible_ranges': [[line_range.start, line_range.end] for line_range in visible_ranges],
        'editable': result.editable,
        'complete_presentation': result.complete_presentation,
        'truncated': bool(reasons),
    }
    if result.observation is not None:
        metadata['observation'] = result.observation.tag
    if reasons:
        metadata['truncation'] = {
            'reasons': reasons,
            'continuations': continuations,
        }
    if unmatched:
        metadata['unmatched_ranges'] = [[line_range.start, line_range.end] for line_range in unmatched]
    return RenderedReadTarget(content='\n'.join(rows), metadata=metadata)


def render_directory_read(
    result: WorkspaceReadDirectoryTreeResult,
    target: PlannedReadTarget,
) -> RenderedReadTarget:
    tree_lines = tuple(_format_directory_line(line) for line in result.lines)
    selected = _select_lines(tree_lines, target.ranges)
    rows = [f'[{result.path}]', *selected]
    available = len(tree_lines)
    remaining = _remaining_ranges(target, available)
    unmatched = tuple(line_range for line_range in target.requested_ranges if line_range.start > available)
    continuations = [format_selector(result.path, remaining)] if remaining else []
    reasons: list[WorkspaceReadTruncationReason] = []

    if available == 0:
        rows.append('[empty directory]')
    if remaining:
        reasons.append('line_limit')
        rows.append(f'[Read limit reached. Continue with {continuations[0]}]')
    if unmatched and not result.scan_truncated:
        rendered = ','.join(_format_range(line_range) for line_range in unmatched)
        rows.append(f'[No entries found for {rendered}; directory tree has {available} lines]')
    if result.limited_directories:
        reasons.append('per_directory_limit')
        rows.append(
            f'[Some directories were abbreviated to {result.child_limit} children. '
            'Read a subdirectory path to expand it]'
        )
    if result.scan_truncated:
        reasons.append('directory_limit')
        rows.append('[Directory scan limit reached. Read a narrower subdirectory]')

    metadata: WorkspaceDirectoryReadToolMetadata = {
        'status': 'ok',
        'kind': 'directory',
        'requested': target.argument,
        'path': result.path,
        'scanned_entries': result.scanned_entries,
        'tree_lines': available,
        'returned_lines': len(selected),
        'per_directory_child_limit': result.child_limit,
        'limited_directories': result.limited_directories,
        'omitted_entries': result.omitted_entries,
        'scan_truncated': result.scan_truncated,
        'truncated': bool(reasons),
    }
    if reasons:
        metadata['truncation'] = {
            'reasons': reasons,
            'continuations': continuations,
        }
    return RenderedReadTarget(content='\n'.join(rows), metadata=metadata)


def render_read_error(argument: str, error: Exception) -> RenderedReadTarget:
    message = str(error)
    metadata: WorkspaceReadErrorToolMetadata = {
        'status': 'error',
        'requested': argument,
        'error': message,
        'error_type': type(error).__name__,
    }
    return RenderedReadTarget(
        content=f'[Could not read {argument}: {message}]',
        metadata=metadata,
    )


def _render_source_lines(lines: tuple[WorkspaceRenderedLine, ...], hashline: bool) -> list[str]:
    rows: list[str] = []
    previous = 0
    for line in lines:
        if previous and line.line_number > previous + 1:
            rows.append('…')
        prefix = f'{line.line_number}:{line.short_hash}|' if hashline else f'{line.line_number}:'
        rows.append(f'{prefix}{line.text}')
        previous = line.line_number
    return rows


def _format_directory_line(line: WorkspaceDirectoryTreeLine) -> str:
    prefix = '  ' * line.depth
    if line.kind == 'omitted':
        return f'{prefix}… {line.omitted_entries} more'
    suffix = '/' if line.kind == 'directory' else ''
    return f'{prefix}{line.name}{suffix}'


def _select_lines(lines: tuple[str, ...], ranges: tuple[ReadLineRange, ...]) -> tuple[str, ...]:
    return tuple(
        lines[number - 1]
        for line_range in ranges
        for number in range(line_range.start, min(line_range.end or len(lines), len(lines)) + 1)
    )


def _expected_lines(ranges: tuple[ReadLineRange, ...], total: int) -> tuple[int, ...]:
    return tuple(
        number for line_range in ranges for number in range(line_range.start, min(line_range.end or total, total) + 1)
    )


def _remaining_ranges(target: PlannedReadTarget, total: int) -> tuple[ReadLineRange, ...]:
    remaining: list[ReadLineRange] = []
    planned_by_start = {line_range.start: line_range for line_range in target.ranges}
    for requested in target.requested_ranges:
        requested_end = total if requested.end is None else min(requested.end, total)
        if requested.start > requested_end:
            continue
        planned = planned_by_start.get(requested.start)
        if planned is None:
            remaining.append(ReadLineRange(start=requested.start, end=requested_end))
            continue
        planned_end = min(planned.end or requested_end, requested_end)
        if planned_end < requested_end:
            remaining.append(ReadLineRange(start=planned_end + 1, end=requested_end))
    return tuple(remaining)


def _line_ranges(numbers: tuple[int, ...]) -> tuple[ReadLineRange, ...]:
    ranges: list[ReadLineRange] = []
    for number in numbers:
        if ranges and ranges[-1].end == number - 1:
            ranges[-1] = ReadLineRange(start=ranges[-1].start, end=number)
        else:
            ranges.append(ReadLineRange(start=number, end=number))
    return tuple(ranges)


def _format_range(line_range: ReadLineRange) -> str:
    return str(line_range.start) if line_range.end is None else f'{line_range.start}-{line_range.end}'
