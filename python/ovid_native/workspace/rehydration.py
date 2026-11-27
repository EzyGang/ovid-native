from collections.abc import Awaitable, Callable, Sequence
from typing import Literal, Self, TypedDict

from ovid_core.models import BaseModel
from pydantic import Field, model_validator


type WorkspaceRehydrationStatus = Literal['restored', 'stale', 'skipped']


class WorkspaceObservationToolMetadata(TypedDict):
    path: str
    tag: str
    content_sha256: str
    visible_ranges: list[list[int]]
    complete_presentation: bool


class WorkspaceRehydrationRequest(BaseModel):
    path: str = Field(min_length=1)
    content_sha256: str = Field(pattern=r'^[0-9a-f]{64}$')
    visible_ranges: tuple[tuple[int, int], ...]
    complete_presentation: bool

    @model_validator(mode='after')
    def validate_ranges(self) -> Self:
        if any(start < 1 or end < start for start, end in self.visible_ranges):
            raise ValueError('visible ranges must be one-based and ordered')
        return self

    @property
    def line_numbers(self) -> tuple[int, ...]:
        return tuple(line for start, end in self.visible_ranges for line in range(start, end + 1))


class WorkspaceRehydrationReport(BaseModel):
    restored: int = Field(ge=0)
    stale: int = Field(ge=0)
    skipped: int = Field(ge=0)


async def rehydrate_newest_first(
    requests: Sequence[WorkspaceRehydrationRequest],
    restore: Callable[[WorkspaceRehydrationRequest], Awaitable[WorkspaceRehydrationStatus]],
) -> WorkspaceRehydrationReport:
    """Restore requests in reverse transcript order and stop at the first request that the ledger cannot retain."""
    counts: dict[WorkspaceRehydrationStatus, int] = {'restored': 0, 'stale': 0, 'skipped': 0}
    for index, request in enumerate(reversed(requests)):
        status = await restore(request)
        if status == 'skipped':
            counts['skipped'] = len(requests) - index
            break
        counts[status] += 1

    return WorkspaceRehydrationReport(restored=counts['restored'], stale=counts['stale'], skipped=counts['skipped'])
