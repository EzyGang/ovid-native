from collections.abc import Mapping
from dataclasses import dataclass, field, replace
from types import TracebackType
from typing import Any, Self

from ovid_core.errors import PluginError
from ovid_core.plugins import PluginActivationContext, PluginServiceFactories
from ovid_core.services import AgentServiceBinding, AgentServices
from pydantic import JsonValue

from ovid_native.workspace.builder import WorkspaceSessionBuilder
from ovid_native.workspace.models import WorkspaceSession
from ovid_native.workspace.operations import workspace_ref


@dataclass(slots=True)
class ActivatedWorkspaceServices:
    services: AgentServices
    _sessions: tuple[WorkspaceSession, ...] = field(repr=False)
    _closed: bool = field(default=False, init=False, repr=False)

    async def __aenter__(self) -> Self:
        return self

    async def __aexit__(
        self,
        exc_type: type[BaseException] | None,
        exc: BaseException | None,
        traceback: TracebackType | None,
    ) -> None:
        del exc_type, exc, traceback
        await self.close()

    async def close(self) -> None:
        if self._closed:
            return

        self._closed = True
        failure: BaseException | None = None
        for session in reversed(self._sessions):
            try:
                await session.close()
            except BaseException as error:
                if failure is None:
                    failure = error

        if failure is not None:
            raise failure


async def activate_workspace_services(
    factories: PluginServiceFactories,
    *,
    context: PluginActivationContext,
    configs: Mapping[str, dict[str, JsonValue]] | None = None,
) -> ActivatedWorkspaceServices:
    selected_configs = {} if configs is None else configs
    _validate_configs(factories, selected_configs)
    bindings = list(context.services.bindings)
    sessions: list[WorkspaceSession] = []

    try:
        for provider in factories.providers:
            services = AgentServices(bindings)
            activation = replace(context, services=services)
            pending = provider.create(context=activation, config=dict(selected_configs.get(provider.id, {})))
            pending = _configure_pending(pending, provider.id, factories, activation, selected_configs)
            session, binding = _build_pending(pending, provider.id)
            sessions.append(session)
            bindings.append(binding)
    except BaseException:
        await _close_sessions(sessions)
        raise

    return ActivatedWorkspaceServices(services=AgentServices(bindings), _sessions=tuple(sessions))


def workspace_builder_binding(
    builder: WorkspaceSessionBuilder,
    *,
    provider_id: str,
    name: str = 'default',
) -> AgentServiceBinding[Any]:
    _validate_factory_id(provider_id)
    return AgentServiceBinding(
        ref=workspace_ref(name),
        value=builder,
        provider=provider_id,
    )


def require_workspace_builder(binding: AgentServiceBinding[Any]) -> WorkspaceSessionBuilder:
    if not isinstance(binding.value, WorkspaceSessionBuilder):
        raise PluginError('Workspace configurator requires an unbuilt WorkspaceSessionBuilder binding')

    return binding.value


def _configure_pending(
    pending: AgentServiceBinding[Any],
    provider_id: str,
    factories: PluginServiceFactories,
    context: PluginActivationContext,
    configs: Mapping[str, dict[str, JsonValue]],
) -> AgentServiceBinding[Any]:
    _validate_pending(pending, provider_id)
    for configurator in factories.configurators:
        if configurator.provider_id != provider_id:
            continue

        configured = configurator.configure(
            pending,
            context=context,
            config=dict(configs.get(configurator.id, {})),
        )
        if configured is not pending:
            raise PluginError(f'Workspace configurator {configurator.id!r} replaced its provider binding')
        _validate_pending(configured, provider_id)

    return pending


def _build_pending(
    pending: AgentServiceBinding[Any],
    provider_id: str,
) -> tuple[WorkspaceSession, AgentServiceBinding[WorkspaceSession]]:
    builder = require_workspace_builder(pending)
    session = builder.build()
    binding = AgentServiceBinding(
        ref=pending.ref,
        value=session,
        provider=provider_id,
        features=frozenset(operation.value for operation in session.operations),
        identity=session.id.root,
    )
    return session, binding


def _validate_pending(binding: AgentServiceBinding[Any], provider_id: str) -> None:
    _validate_factory_id(provider_id)
    if binding.provider != provider_id:
        raise PluginError(f'Workspace provider {provider_id!r} returned a mismatched provider binding')
    if binding.ref.key != workspace_ref(binding.ref.name).key:
        raise PluginError(f'Workspace provider {provider_id!r} returned a non-workspace service binding')

    require_workspace_builder(binding)


def _validate_configs(
    factories: PluginServiceFactories,
    configs: Mapping[str, dict[str, JsonValue]],
) -> None:
    identifiers = {provider.id for provider in factories.providers}
    identifiers.update(configurator.id for configurator in factories.configurators)
    unknown = sorted(set(configs) - identifiers)
    if unknown:
        raise PluginError(f'Configuration supplied for unselected plugin factory: {unknown[0]!r}')


def _validate_factory_id(identifier: str) -> None:
    if '.' not in identifier:
        raise PluginError(f'Workspace plugin factory ID must be globally namespaced: {identifier!r}')


async def _close_sessions(sessions: list[WorkspaceSession]) -> None:
    for session in reversed(sessions):
        try:
            await session.close()
        except BaseException:
            pass
