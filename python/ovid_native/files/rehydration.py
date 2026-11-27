from collections.abc import Iterable

from pydantic import JsonValue, ValidationError

from ovid_native.workspace.rehydration import WorkspaceRehydrationRequest


def receipts_from_tool_results(metadata: Iterable[JsonValue]) -> tuple[WorkspaceRehydrationRequest, ...]:
    """Return requests in transcript order and ignore values without a valid `observations` list."""
    requests: list[WorkspaceRehydrationRequest] = []
    for value in metadata:
        observations = value.get('observations') if isinstance(value, dict) else None
        if not isinstance(observations, list):
            continue
        for observation in observations:
            if not isinstance(observation, dict):
                continue
            try:
                requests.append(
                    WorkspaceRehydrationRequest.model_validate(
                        {name: observation.get(name) for name in WorkspaceRehydrationRequest.model_fields}
                    )
                )
            except ValidationError:
                continue

    return tuple(requests)
