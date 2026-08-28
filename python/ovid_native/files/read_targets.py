import re
from dataclasses import dataclass

from ovid_native.files.models import ReadLineRange
from ovid_native.workspace.errors import WorkspaceReadError


DEFAULT_READ_LINES = 300
MAX_READ_LINES = 3_000
MAX_READ_RANGES = 32
MAX_READ_TARGETS = 32

_RANGE_PART = re.compile(r'^(?P<start>[1-9]\d*)(?:(?P<open>-)|-(?P<end>[1-9]\d*)|\+(?P<count>[1-9]\d*))?$')


@dataclass(frozen=True, slots=True)
class ParsedReadTarget:
    argument: str
    path: str
    ranges: tuple[ReadLineRange, ...]


@dataclass(frozen=True, slots=True)
class PlannedReadTarget:
    argument: str
    path: str
    requested_ranges: tuple[ReadLineRange, ...]
    ranges: tuple[ReadLineRange, ...]


def split_read_targets(argument: str) -> tuple[str, ...]:
    targets = tuple(part.strip() for part in argument.split(';'))
    if any(not target for target in targets):
        raise WorkspaceReadError('Read targets separated by semicolons cannot be empty')
    if len(targets) > MAX_READ_TARGETS:
        raise WorkspaceReadError(f'Read accepts at most {MAX_READ_TARGETS} targets in one call')
    return targets


def parse_read_target(argument: str, fallback_ranges: tuple[ReadLineRange, ...] = ()) -> ParsedReadTarget:
    path, separator, selector = argument.rpartition(':')
    parsed_ranges = _parse_selector(selector) if separator else None
    if parsed_ranges is None:
        return ParsedReadTarget(
            argument=argument,
            path=argument,
            ranges=fallback_ranges,
        )
    if not path:
        raise WorkspaceReadError(f"Read selector '{argument}' does not include a path")
    if fallback_ranges:
        raise WorkspaceReadError('Use either inline read selectors or the ranges field, not both')
    return ParsedReadTarget(
        argument=argument,
        path=path,
        ranges=parsed_ranges,
    )


def plan_read_target(target: ParsedReadTarget, line_budget: int) -> PlannedReadTarget:
    source_ranges = target.ranges or (ReadLineRange(start=1),)
    remaining = min(line_budget, MAX_READ_LINES)
    planned: list[ReadLineRange] = []

    for line_range in source_ranges:
        requested_count = DEFAULT_READ_LINES if line_range.end is None else line_range.end - line_range.start + 1
        selected_count = min(requested_count, remaining)
        if selected_count > 0:
            planned.append(ReadLineRange(start=line_range.start, end=line_range.start + selected_count - 1))
        remaining -= selected_count

    return PlannedReadTarget(
        argument=target.argument,
        path=target.path,
        requested_ranges=source_ranges,
        ranges=tuple(planned),
    )


def format_selector(path: str, ranges: tuple[ReadLineRange, ...]) -> str:
    rendered = ','.join(
        str(line_range.start) if line_range.end is None else f'{line_range.start}-{line_range.end}'
        for line_range in ranges
    )
    return f'{path}:{rendered}'


def _parse_selector(selector: str) -> tuple[ReadLineRange, ...] | None:
    if not selector or not selector[0].isdigit():
        return None
    parts = selector.split(',')
    if len(parts) > MAX_READ_RANGES:
        raise WorkspaceReadError(f'Read selectors accept at most {MAX_READ_RANGES} ranges')

    ranges = tuple(_parse_range(part, selector) for part in parts)
    _reject_overlapping_ranges(ranges)
    return ranges


def _parse_range(part: str, selector: str) -> ReadLineRange:
    match = _RANGE_PART.fullmatch(part)
    if match is None:
        raise WorkspaceReadError(
            f"Invalid read selector ':{selector}'. Use :N, :N-M, :N+K, :N-, or comma-separated ranges"
        )
    start = int(match.group('start'))
    end_text = match.group('end')
    count_text = match.group('count')
    if end_text is not None:
        end = int(end_text)
    elif count_text is not None:
        end = start + int(count_text) - 1
    else:
        end = None
    try:
        return ReadLineRange(start=start, end=end)
    except ValueError as error:
        raise WorkspaceReadError(f"Invalid read selector ':{selector}': {error}") from error


def _reject_overlapping_ranges(ranges: tuple[ReadLineRange, ...]) -> None:
    ordered = sorted(ranges, key=lambda line_range: line_range.start)
    for previous, current in zip(ordered, ordered[1:], strict=False):
        if previous.end is None or current.start <= previous.end:
            raise WorkspaceReadError('Read selector ranges cannot overlap')
