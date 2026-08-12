import argparse
from collections import defaultdict
from pathlib import Path

import pyperf

from benchmarks.history import DATA_ROOT, RESULTS_PATH


def main() -> None:
    parser = argparse.ArgumentParser(description='Generate committed ovid-native benchmark history')
    parser.add_argument('--output', type=Path, default=RESULTS_PATH)
    args = parser.parse_args()

    records = sorted(path for path in DATA_ROOT.glob('v*/*/*.json'))
    groups: dict[str, list[tuple[str, Path, pyperf.BenchmarkSuite]]] = defaultdict(list)
    for path in records:
        with path.open(encoding='utf-8') as record:
            suite = pyperf.BenchmarkSuite.load(record)
        metadata = suite.get_metadata()
        machine = str(metadata.get('ovid_machine_key', 'unknown'))
        version = str(metadata.get('ovid_package_version', path.parent.name))
        groups[machine].append((version, path, suite))

    lines = ['# Benchmark History', '']
    if not groups:
        lines.extend(
            (
                'No accepted release benchmark records yet.',
                '',
                'Run `uv run task benchmark-record -- --record-version <version>` on a fixed machine.',
            )
        )
    for machine, suites in sorted(groups.items()):
        lines.extend((f'## `{machine}`', ''))
        versions = [version for version, _, _ in suites]
        scenario_names = sorted({name for _, _, suite in suites for name in suite.get_benchmark_names()})
        lines.append('| Scenario | ' + ' | '.join(versions) + ' |')
        lines.append('| --- | ' + ' | '.join('---:' for _ in versions) + ' |')
        for name in scenario_names:
            values = []
            for _, _, suite in suites:
                try:
                    benchmark = suite.get_benchmark(name)
                except KeyError:
                    values.append('—')
                else:
                    values.append(_format_benchmark(benchmark))
            lines.append(f'| `{name}` | ' + ' | '.join(values) + ' |')
        lines.append('')
        lines.append('Records:')
        for version, path, _ in suites:
            lines.append(f'- `{version}`: `{path.relative_to(Path(__file__).parent)}`')
        lines.append('')

    args.output.write_text('\n'.join(lines).rstrip() + '\n', encoding='utf-8')
    print(f'Generated {args.output}')


def _format_benchmark(benchmark: pyperf.Benchmark) -> str:
    median = benchmark.median()
    metadata = benchmark.get_metadata()
    work_items = metadata.get('ovid_work_items')
    work_unit = metadata.get('ovid_work_unit')
    timing = f'{median * 1_000:.3f} ms'
    if not isinstance(work_items, int) or not isinstance(work_unit, str):
        return timing

    throughput = work_items / median
    return f'{timing} ({throughput:,.0f} {work_unit}/s)'


if __name__ == '__main__':
    main()
