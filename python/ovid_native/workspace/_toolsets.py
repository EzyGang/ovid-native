from abc import abstractmethod
from collections.abc import Awaitable, Callable, Sequence
from typing import TYPE_CHECKING, Any

from ovid_core.models import BaseModel
from ovid_core.runtime.context import RunContext
from ovid_core.tools.base import BaseTool, BaseToolset, ToolExecutionContext
from ovid_core.tools.models import ToolResult

from ovid_native.workspace.evidence import EditableSourceGroup, WorkspaceSourcePresenter, capture_source_presentation
from ovid_native.workspace.observations import WorkspaceObservationService


if TYPE_CHECKING:
    from ovid_native.files.edit_modes import EditModeState


class WorkspaceResultTool[Deps, Provider, Args: BaseModel, Result: ToolResult](BaseTool[Deps, Args, Result]):
    def __init__(self, *, provider: Provider) -> None:
        self._provider = provider

    async def execute(self, context: ToolExecutionContext[Deps], arguments: Args) -> Result:
        del context
        content = await self._content(arguments)
        return self.result_type(content=content.model_dump(mode='json'))

    @abstractmethod
    async def _content(self, arguments: Args) -> BaseModel: ...


class WorkspaceSourceTool[Deps, Provider, Args: BaseModel, Result: ToolResult](BaseTool[Deps, Args, Result]):
    def __init__(self, *, provider: Provider, presenter: WorkspaceSourcePresenter | None = None) -> None:
        self._provider = provider
        self._presenter = presenter

    async def _present_result[Data: BaseModel](
        self,
        result: Data,
        observe: Callable[[Data, WorkspaceSourcePresenter], Awaitable[tuple[EditableSourceGroup, ...]]],
    ) -> Result:
        metadata = {'result': result.model_dump(mode='json')}
        if self._presenter is None:
            return self.result_type(content=metadata)
        groups = await observe(result, self._presenter)
        if self._presenter.presentation.format == 'hashline':
            content = '\n\n'.join(group.render(self._presenter.presentation) for group in groups)
            return self.result_type(content=content, metadata=metadata)
        return self.result_type(content=metadata)


class WorkspaceStepToolset[Deps](BaseToolset[Deps]):
    @abstractmethod
    async def for_step(self, context: RunContext[Deps]) -> BaseToolset[Deps]: ...

    async def get_tools(self, context: RunContext[Deps]) -> Sequence[BaseTool[Deps, Any, Any]]:
        return await (await self.for_step(context)).get_tools(context)


class WorkspaceSourceToolset[Deps, Provider](WorkspaceStepToolset[Deps]):
    def __init__(
        self,
        *,
        provider: Provider,
        state: EditModeState,
        observations: WorkspaceObservationService,
    ) -> None:
        self._provider = provider
        self._state = state
        self._observations = observations

    async def for_step(self, context: RunContext[Deps]) -> BaseToolset[Deps]:
        del context
        selection = self._state.current
        presenter = WorkspaceSourcePresenter(
            observations=self._observations,
            presentation=capture_source_presentation(selection.mode, selection.generation),
        )
        return BoundWorkspaceToolset(owner=self, tools=self._source_tools(presenter))

    @abstractmethod
    def _source_tools(self, presenter: WorkspaceSourcePresenter) -> Sequence[BaseTool[Deps, Any, Any]]: ...


class BoundWorkspaceToolset[Deps](BaseToolset[Deps]):
    def __init__(self, *, owner: BaseToolset[Deps], tools: Sequence[BaseTool[Deps, Any, Any]]) -> None:
        self.id = owner.id
        self._owner = owner
        self._tools = tuple(tools)

    async def for_step(self, context: RunContext[Deps]) -> BaseToolset[Deps]:
        return await self._owner.for_step(context)

    async def get_tools(self, context: RunContext[Deps]) -> Sequence[BaseTool[Deps, Any, Any]]:
        del context
        return self._tools
