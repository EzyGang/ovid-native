from dataclasses import dataclass
from typing import Literal, cast

from ovid_native import _native
from ovid_native.files.models import (
    WorkspaceEditResult,
    WorkspaceFileChange,
    WorkspacePostEditSource,
    WorkspaceWriteResult,
)
from ovid_native.workspace.models import WorkspaceSessionId
from ovid_native.workspace.observations import (
    NativeWorkspaceChangeEvents,
    WorkspaceObservationReceipt,
    WorkspaceRenderedLine,
)


@dataclass(slots=True)
class WorkspaceEditResultMapper:
    _session_id: WorkspaceSessionId
    _change_events: NativeWorkspaceChangeEvents

    def _write_result(self, native: _native.NativeWorkspaceEditResult) -> WorkspaceWriteResult:
        result = self._edit_result(native)
        return WorkspaceWriteResult.model_validate(result.model_dump())

    def _edit_result(self, native: _native.NativeWorkspaceEditResult) -> WorkspaceEditResult:
        mode, mode_generation, policy_generation, changes, posts, preflight, commit, strategy, confidence = native
        mapped_changes = tuple(self._change(change) for change in changes)
        result = WorkspaceEditResult(
            mode=mode,
            mode_generation=mode_generation,
            policy_generation=policy_generation,
            changes=mapped_changes,
            post_edit_sources=tuple(self._post_source(post) for post in posts),
            preflight_complete=preflight,
            commit_complete=commit,
            matching_strategy=cast(Literal['exact', 'fuzzy'] | None, strategy),
            confidence=confidence,
        )
        self._publish_changes(mapped_changes)
        return result

    def _change(self, native: _native.NativeWorkspaceFileChange) -> WorkspaceFileChange:
        path, operation, destination, before, after, receipt, generation, revision = native
        return WorkspaceFileChange(
            path=path,
            operation=cast(Literal['create', 'update', 'delete', 'move'], operation),
            destination=destination,
            before_sha256=before,
            after_sha256=after,
            observation=None if receipt is None else self._receipt(receipt),
            file_generation=generation,
            revision=revision,
        )

    def _post_source(self, native: _native.NativeWorkspacePostEditSource) -> WorkspacePostEditSource:
        path, receipt, lines, complete = native
        return WorkspacePostEditSource(
            path=path,
            observation=self._receipt(receipt),
            lines=tuple(rendered_line(line) for line in lines),
            complete_presentation=complete,
        )

    def _receipt(self, native: _native.NativeWorkspaceObservationReceipt) -> WorkspaceObservationReceipt:
        path, tag, content_sha256, generation, ranges, complete = native
        return WorkspaceObservationReceipt(
            session_id=self._session_id,
            path=path,
            tag=tag,
            content_sha256=content_sha256,
            generation=generation,
            visible_ranges=tuple({'start': start, 'end': end} for start, end in ranges),
            complete_presentation=complete,
        )

    def _publish_changes(self, changes: tuple[WorkspaceFileChange, ...]) -> None:
        for change in changes:
            self._change_events.publish(
                path=change.path,
                operation=change.operation,
                destination=change.destination,
                generation=change.file_generation,
                revision=change.revision,
            )


def rendered_line(native: _native.NativeWorkspaceRenderedLine) -> WorkspaceRenderedLine:
    return WorkspaceRenderedLine(line_number=native[0], short_hash=native[1], text=native[2])
