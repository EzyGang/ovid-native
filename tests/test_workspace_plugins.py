import asyncio
from pathlib import Path
from typing import Any, cast

import pytest
from ovid_core.errors import PluginError
from ovid_core.plugins import PluginActivationContext, PluginRegistrar
from ovid_core.services import AgentServiceBinding, AgentServiceKey, AgentServiceRef, AgentServices
from pydantic import JsonValue
from pytest_mock import MockerFixture

from ovid_native.workspace.builder import WorkspaceSessionBuilder
from ovid_native.workspace.errors import WorkspaceClosedError
from ovid_native.workspace.models import WorkspaceSession, WorkspaceSessionId
from ovid_native.workspace.operations import workspace_ref
from ovid_native.workspace.plugins import (
    ActivatedWorkspaceServices,
    activate_workspace_services,
    require_workspace_builder,
    workspace_builder_binding,
)


def test_plugin_activation_configures_builder_before_freeze_and_owns_lifecycle(tmp_path: Path) -> None:
    events: list[str] = []

    def provide_workspace(
        *,
        context: PluginActivationContext,
        config: dict[str, JsonValue],
    ) -> AgentServiceBinding[Any]:
        assert context.services.bindings == ()
        assert config == {'selected': True}
        events.append('provider')
        return workspace_builder_binding(
            WorkspaceSessionBuilder.native(root=tmp_path),
            provider_id='example.workspace',
        )

    def configure_workspace(
        binding: AgentServiceBinding[Any],
        *,
        context: PluginActivationContext,
        config: dict[str, JsonValue],
    ) -> AgentServiceBinding[Any]:
        assert context.services.bindings == ()
        assert config == {'mode': 'replace'}
        events.append('configurator')
        require_workspace_builder(binding).with_edit_mode('replace')
        return binding

    async def run() -> None:
        registrar = PluginRegistrar()
        registrar.register_service_provider_factory(id='example.workspace', factory=provide_workspace)
        registrar.register_service_configurator_factory(
            id='example.workspace.configure',
            provider_id='example.workspace',
            factory=configure_workspace,
        )
        selected = registrar.select_service_factories(
            providers=('example.workspace',),
            configurators=('example.workspace.configure',),
        )
        activated = await activate_workspace_services(
            selected,
            context=PluginActivationContext(services=AgentServices()),
            configs={
                'example.workspace': {'selected': True},
                'example.workspace.configure': {'mode': 'replace'},
            },
        )
        session = activated.services.resolve(workspace_ref())

        assert events == ['provider', 'configurator']
        assert session.edit_mode.current.mode == 'replace'
        assert activated.services.binding(workspace_ref()).provider == 'example.workspace'
        assert activated.services.binding(workspace_ref()).identity == session.id.root

        await activated.close()
        await activated.close()
        with pytest.raises(WorkspaceClosedError):
            _ = session.edit_mode.current

    asyncio.run(run())


def test_plugin_activation_rejects_binding_replacement_and_unknown_configuration(tmp_path: Path) -> None:
    def provide_workspace(
        *,
        context: PluginActivationContext,
        config: dict[str, JsonValue],
    ) -> AgentServiceBinding[Any]:
        del context, config
        return workspace_builder_binding(
            WorkspaceSessionBuilder.native(root=tmp_path),
            provider_id='example.workspace',
        )

    def replace_workspace(
        binding: AgentServiceBinding[Any],
        *,
        context: PluginActivationContext,
        config: dict[str, JsonValue],
    ) -> AgentServiceBinding[Any]:
        del context, config
        return AgentServiceBinding(
            ref=binding.ref,
            value=WorkspaceSessionBuilder.native(root=tmp_path),
            provider=binding.provider,
        )

    async def run() -> None:
        registrar = PluginRegistrar()
        registrar.register_service_provider_factory(id='example.workspace', factory=provide_workspace)
        registrar.register_service_configurator_factory(
            id='example.workspace.replace',
            provider_id='example.workspace',
            factory=replace_workspace,
        )
        selected = registrar.select_service_factories(
            providers=('example.workspace',),
            configurators=('example.workspace.replace',),
        )

        with pytest.raises(PluginError, match='replaced its provider binding'):
            await activate_workspace_services(
                selected,
                context=PluginActivationContext(services=AgentServices()),
            )
        with pytest.raises(PluginError, match='unselected plugin factory'):
            await activate_workspace_services(
                registrar.select_service_factories(providers=('example.workspace',)),
                context=PluginActivationContext(services=AgentServices()),
                configs={'example.missing': {}},
            )

    asyncio.run(run())


def test_plugin_adapter_validates_boundaries_and_closes_failures(tmp_path: Path, mocker: MockerFixture) -> None:
    async def run() -> None:
        managed_session = mocker.Mock()
        managed_session.close = mocker.AsyncMock()
        managed = ActivatedWorkspaceServices(
            services=AgentServices(),
            _sessions=(cast(WorkspaceSession, managed_session),),
        )
        async with managed as entered:
            assert entered is managed
        managed_session.close.assert_awaited_once()

        first_session = mocker.Mock()
        first_session.close = mocker.AsyncMock(side_effect=RuntimeError('later failure'))
        failing_session = mocker.Mock()
        failing_session.close = mocker.AsyncMock(side_effect=RuntimeError('close failed'))
        failing = ActivatedWorkspaceServices(
            services=AgentServices(),
            _sessions=(cast(WorkspaceSession, first_session), cast(WorkspaceSession, failing_session)),
        )
        with pytest.raises(RuntimeError, match='close failed'):
            await failing.close()
        first_session.close.assert_awaited_once()
        await failing.close()

        with pytest.raises(PluginError, match='unbuilt'):
            require_workspace_builder(
                AgentServiceBinding(ref=workspace_ref(), value='built', provider='example.workspace')
            )
        with pytest.raises(PluginError, match='globally namespaced'):
            workspace_builder_binding(WorkspaceSessionBuilder.native(root=tmp_path), provider_id='workspace')

        mismatched = PluginRegistrar()
        mismatched.register_service_provider_factory(
            id='example.workspace',
            factory=mocker.Mock(
                return_value=AgentServiceBinding(
                    ref=workspace_ref(),
                    value=WorkspaceSessionBuilder.native(root=tmp_path),
                    provider='example.other',
                )
            ),
        )
        with pytest.raises(PluginError, match='mismatched provider binding'):
            await activate_workspace_services(
                mismatched.select_service_factories(providers=('example.workspace',)),
                context=PluginActivationContext(services=AgentServices()),
            )

        wrong_service = PluginRegistrar()
        wrong_service.register_service_provider_factory(
            id='example.workspace',
            factory=mocker.Mock(
                return_value=AgentServiceBinding(
                    ref=AgentServiceRef(key=AgentServiceKey(id='example.other', api_version=1)),
                    value=WorkspaceSessionBuilder.native(root=tmp_path),
                    provider='example.workspace',
                )
            ),
        )
        with pytest.raises(PluginError, match='non-workspace service binding'):
            await activate_workspace_services(
                wrong_service.select_service_factories(providers=('example.workspace',)),
                context=PluginActivationContext(services=AgentServices()),
            )

        skipped_configurator = mocker.Mock(side_effect=lambda binding, **_kwargs: binding)
        skipped = PluginRegistrar()
        skipped.register_service_provider_factory(
            id='example.workspace',
            factory=mocker.Mock(
                return_value=workspace_builder_binding(
                    WorkspaceSessionBuilder.native(root=tmp_path),
                    provider_id='example.workspace',
                    name='first',
                )
            ),
        )
        skipped.register_service_provider_factory(
            id='example.other',
            factory=mocker.Mock(
                return_value=workspace_builder_binding(
                    WorkspaceSessionBuilder.native(root=tmp_path),
                    provider_id='example.other',
                    name='second',
                )
            ),
        )
        skipped.register_service_configurator_factory(
            id='example.other.configure',
            provider_id='example.other',
            factory=skipped_configurator,
        )
        activated = await activate_workspace_services(
            skipped.select_service_factories(
                providers=('example.workspace', 'example.other'),
                configurators=('example.other.configure',),
            ),
            context=PluginActivationContext(services=AgentServices()),
        )
        skipped_configurator.assert_called_once()
        await activated.close()

        cleanup_session = mocker.Mock()
        cleanup_session.id = WorkspaceSessionId('cleanup')
        cleanup_session.operations = frozenset()
        cleanup_session.close = mocker.AsyncMock(side_effect=RuntimeError('cleanup failed'))
        mocker.patch.object(WorkspaceSessionBuilder, 'build', return_value=cleanup_session)
        cleanup = PluginRegistrar()
        cleanup.register_service_provider_factory(
            id='example.first',
            factory=mocker.Mock(
                return_value=workspace_builder_binding(
                    WorkspaceSessionBuilder.native(root=tmp_path),
                    provider_id='example.first',
                    name='first',
                )
            ),
        )
        cleanup.register_service_provider_factory(
            id='example.second',
            factory=mocker.Mock(side_effect=RuntimeError('provider failed')),
        )
        with pytest.raises(RuntimeError, match='provider failed'):
            await activate_workspace_services(
                cleanup.select_service_factories(providers=('example.first', 'example.second')),
                context=PluginActivationContext(services=AgentServices()),
            )
        cleanup_session.close.assert_awaited_once()

    asyncio.run(run())
