import asyncio
from collections.abc import Awaitable, Callable, Sequence
from dataclasses import dataclass
from pathlib import Path
from time import perf_counter

from benchmarks.ast_scenarios import ast_benchmark_cases
from benchmarks.fff_scenarios import fff_benchmark_cases
from benchmarks.search_scenarios import search_benchmark_cases


type Measure = Callable[[int], float]
type AsyncMeasure = Callable[[Callable[[], Awaitable[object]]], Measure]
type BenchmarkCase = tuple[str, str, str, Measure, int, str]
type BenchmarkCases = tuple[tuple[BenchmarkCase, ...], Callable[[], None]]


@dataclass(frozen=True, slots=True)
class Scenario:
    name: str
    operation: str
    purpose: str
    measure: Callable[[int], float]
    work_items: int
    work_unit: str


@dataclass(frozen=True, slots=True)
class ScenarioSuite:
    cases: tuple[Scenario, ...]
    close: Callable[[], None]


def scenarios(fixture_root: Path, work_root: Path) -> ScenarioSuite:
    loop = asyncio.new_event_loop()

    def async_measure(operation: Callable[[], Awaitable[object]]) -> Callable[[int], float]:
        def measure(loops: int) -> float:
            started = perf_counter()
            for _ in range(loops):
                loop.run_until_complete(operation())
            return perf_counter() - started

        return measure

    benchmark_cases = [
        Scenario(*case)
        for case in (
            *search_benchmark_cases(fixture_root, async_measure),
            *ast_benchmark_cases(fixture_root, work_root, loop, async_measure),
        )
    ]
    fff_cases, close = fff_benchmark_cases(fixture_root, loop, async_measure)
    benchmark_cases.extend(Scenario(*case) for case in fff_cases)

    return ScenarioSuite(cases=tuple(benchmark_cases), close=close)


def select_scenarios(all_scenarios: Sequence[Scenario], patterns: Sequence[str]) -> tuple[Scenario, ...]:
    if not patterns:
        return tuple(all_scenarios)

    selected = tuple(scenario for scenario in all_scenarios if any(pattern in scenario.name for pattern in patterns))
    if not selected:
        raise RuntimeError(f'No benchmark scenarios match: {", ".join(patterns)}')
    return selected
