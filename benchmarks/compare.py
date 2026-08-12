import argparse
import math
from pathlib import Path

import pyperf
from pyperf._utils import is_significant

from benchmarks.history import comparison_key


REGRESSION_PERCENT = 10.0
REGRESSION_SECONDS = 0.002


def main() -> None:
    parser = argparse.ArgumentParser(description='Compare equivalent ovid-native pyperf records')
    parser.add_argument('reference', type=Path)
    parser.add_argument('current', type=Path)
    parser.add_argument('--output', type=Path)
    args = parser.parse_args()

    with args.reference.open(encoding='utf-8') as reference_file:
        reference = pyperf.BenchmarkSuite.load(reference_file)
    with args.current.open(encoding='utf-8') as current_file:
        current = pyperf.BenchmarkSuite.load(current_file)
    _validate_compatibility(reference, current)
    report, regressions = compare(reference, current)
    print(report)
    if args.output:
        args.output.write_text(report + '\n', encoding='utf-8')
    if regressions:
        raise SystemExit(1)


def compare(reference: pyperf.BenchmarkSuite, current: pyperf.BenchmarkSuite) -> tuple[str, int]:
    reference_names = set(reference.get_benchmark_names())
    current_names = set(current.get_benchmark_names())
    if reference_names != current_names:
        missing = ', '.join(sorted(reference_names - current_names)) or 'none'
        added = ', '.join(sorted(current_names - reference_names)) or 'none'
        raise RuntimeError(f'Scenario sets differ; missing: {missing}; added: {added}')

    lines = [
        '| Scenario | Reference | Current | Change | Status |',
        '| --- | ---: | ---: | ---: | --- |',
    ]
    regressions = 0
    for name in sorted(reference_names):
        previous = reference.get_benchmark(name)
        candidate = current.get_benchmark(name)
        previous_median = previous.median()
        candidate_median = candidate.median()
        change = (candidate_median / previous_median - 1.0) * 100
        delta = candidate_median - previous_median
        significant = _is_significant(previous.get_values(), candidate.get_values())
        if significant and change >= REGRESSION_PERCENT and delta >= REGRESSION_SECONDS:
            status = 'regression'
            regressions += 1
        elif significant and change <= -REGRESSION_PERCENT and delta <= -REGRESSION_SECONDS:
            status = 'improved'
        elif not significant:
            status = 'unstable'
        else:
            status = 'stable'
        lines.append(
            f'| `{name}` | {_milliseconds(previous_median)} | {_milliseconds(candidate_median)} '
            f'| {change:+.1f}% | {status} |'
        )

    return '\n'.join(lines), regressions


def _validate_compatibility(
    reference: pyperf.BenchmarkSuite,
    current: pyperf.BenchmarkSuite,
) -> None:
    reference_metadata = reference.get_metadata()
    current_metadata = current.get_metadata()
    if comparison_key(reference_metadata) != comparison_key(current_metadata):
        raise RuntimeError('Benchmark records are not comparable: suite, fixture, machine, or profile differs')


def _is_significant(reference: tuple[float, ...], current: tuple[float, ...]) -> bool:
    if len(reference) < 2 or len(current) < 2:
        return False
    significant, _ = is_significant(reference, current)
    return significant


def _milliseconds(seconds: float) -> str:
    milliseconds = seconds * 1_000
    if not math.isfinite(milliseconds):
        raise RuntimeError('Benchmark contains a non-finite median')
    return f'{milliseconds:.3f} ms'


if __name__ == '__main__':
    main()
