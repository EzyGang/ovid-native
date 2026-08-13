import asyncio
from pathlib import Path

import pytest
from pytest_mock import MockerFixture

from ovid_native.fff import (
    FffClosedError,
    FffConfig,
    FffEngine,
    FffFindRequest,
    FffGrepRequest,
    FffMultiGrepRequest,
    FffPatternError,
    FffRuntimeError,
)


def _write_fixture(root: Path) -> None:
    (root / 'credential_resolver.py').write_text('class CredentialResolver:\n    pass\n')
    (root / 'variants.txt').write_text('credential_resolver\ncredentialResolver\na+b\n')


def test_engine_searches_and_closes(tmp_path: Path) -> None:
    async def run() -> None:
        _write_fixture(tmp_path)
        engine = FffEngine(root=tmp_path, config=FffConfig(watch=False))
        assert engine.config == FffConfig(watch=False)
        ready = await engine.wait_ready(timeout_seconds=10.0)
        await engine.rescan()
        ready = await engine.wait_ready()
        found = await engine.find(FffFindRequest(query='credentail resolver'))
        grep = await engine.grep(FffGrepRequest(query='CredentialResolver', mode='plain'))
        multi = await engine.multi_grep(FffMultiGrepRequest(patterns=('credential_resolver', 'a+b')))
        await engine.close()

        assert ready.state == 'ready'
        assert found.matches[0].path == 'credential_resolver.py'
        assert grep.matches[0].column == 7
        assert grep.matches[0].is_definition
        assert len(multi.matches) == 2
        assert (await engine.status()).state == 'closed'
        with pytest.raises(FffClosedError):
            await engine.find(FffFindRequest(query='resolver'))

    asyncio.run(run())


def test_invalid_regex_is_not_literal_fallback(tmp_path: Path) -> None:
    async def run() -> None:
        _write_fixture(tmp_path)
        async with FffEngine(root=tmp_path, config=FffConfig(watch=False)) as engine:
            with pytest.raises(FffPatternError):
                await engine.grep(FffGrepRequest(query='[', mode='regex'))

    asyncio.run(run())


def test_context_manager_does_not_block_event_loop(tmp_path: Path) -> None:
    async def run() -> None:
        _write_fixture(tmp_path)
        ticks: list[int] = []

        async def tick() -> None:
            await asyncio.sleep(0)
            ticks.append(1)

        engine = FffEngine(root=tmp_path, config=FffConfig(watch=False))
        await asyncio.gather(engine.wait_ready(), tick())
        await engine.close()

        assert ticks == [1]

    asyncio.run(run())


def test_runtime_errors_map_to_public_type(tmp_path: Path, mocker: MockerFixture) -> None:
    engine = FffEngine(root=tmp_path, config=FffConfig(watch=False))
    failure = mocker.patch('ovid_native.fff.engine._native.fff_status')
    native_error = __import__(
        'ovid_native._native',
        fromlist=['NativeFffRuntimeError'],
    ).NativeFffRuntimeError
    failure.side_effect = native_error('runtime failed')

    with pytest.raises(FffRuntimeError, match='runtime failed'):
        asyncio.run(engine.status())
