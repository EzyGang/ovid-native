import argparse
import atexit
import os
import shutil
import tempfile
from pathlib import Path

import pyperf

from benchmarks.fixtures import build_fixtures, validate_fixtures
from benchmarks.history import environment, rebuild_index, sanitize_result, validate_recording
from benchmarks.scenarios import scenarios, select_scenarios


_TEMPORARY_ROOT: Path | None = None
_FIXTURE_DIGEST: str | None = None
_FIXTURE_ROOT: Path | None = None
_WORK_ROOT: Path | None = None


def main() -> None:
    parser = argparse.ArgumentParser(description='Run ovid-native public API benchmarks')
    parser.add_argument('--scenario', action='append', default=[], help='Scenario name substring to run')
    parser.add_argument('--fixture-root', type=Path, help=argparse.SUPPRESS)
    parser.add_argument('--record-version', help='Persist an immutable release benchmark record')
    parser.add_argument('--allow-dirty', action='store_true', help='Allow recording from a dirty worktree')
    parser.add_argument(
        '--release-extension', action='store_true', help='Assert the installed extension is release-built'
    )
    runner = pyperf.Runner(
        _argparser=parser,
        add_cmdline_args=_add_worker_arguments,
        program_args=('-m', 'benchmarks.run'),
    )
    args = runner.parse_args()

    profile = 'release' if args.release_extension else 'development'
    current = environment(profile)
    destination = None
    pending = None
    if args.record_version and not args.worker:
        destination = validate_recording(args.record_version, current, allow_dirty=args.allow_dirty)
        pending = destination.with_name(f'.{destination.name}.{os.getpid()}.pending')
        if pending.exists():
            raise RuntimeError(f'Pending benchmark result already exists: {pending}')
        args.output = str(pending)

    fixture_root, work_root, fixture_digest = _prepare_workspace(args.fixture_root)
    args.fixture_root = fixture_root
    suite = scenarios(fixture_root, work_root)
    try:
        selected = select_scenarios(suite.cases, args.scenario)
        for scenario in selected:
            metadata = {
                **current.pyperf_metadata(fixture_digest),
                'ovid_operation': scenario.operation,
                'ovid_purpose': scenario.purpose,
                'ovid_work_items': scenario.work_items,
                'ovid_work_unit': scenario.work_unit,
            }
            runner.bench_time_func(scenario.name, scenario.measure, metadata=metadata)
    finally:
        suite.close()
    if destination is not None and pending is not None and not args.worker and pending.exists():
        sanitize_result(pending)
        if destination.exists():
            raise RuntimeError(f'Benchmark result already exists: {destination}')
        pending.rename(destination)
        rebuild_index()
        print(f'Recorded benchmark: {destination}')


def _add_worker_arguments(command: list[str], args: argparse.Namespace) -> None:
    for scenario in args.scenario:
        command.extend(('--scenario', scenario))
    command.extend(('--fixture-root', str(args.fixture_root)))
    if args.release_extension:
        command.append('--release-extension')


def _prepare_workspace(supplied_fixture_root: Path | None) -> tuple[Path, Path, str]:
    global _FIXTURE_DIGEST, _FIXTURE_ROOT, _TEMPORARY_ROOT, _WORK_ROOT

    if _FIXTURE_ROOT is not None and _WORK_ROOT is not None and _FIXTURE_DIGEST is not None:
        return _FIXTURE_ROOT, _WORK_ROOT, _FIXTURE_DIGEST

    _TEMPORARY_ROOT = Path(tempfile.mkdtemp(prefix='ovid-native-benchmark-'))
    _WORK_ROOT = _TEMPORARY_ROOT / 'work'
    _WORK_ROOT.mkdir()
    if supplied_fixture_root is None:
        _FIXTURE_ROOT = _TEMPORARY_ROOT / 'fixtures'
        build_fixtures(_FIXTURE_ROOT)
    else:
        _FIXTURE_ROOT = supplied_fixture_root.resolve()
    _FIXTURE_DIGEST = validate_fixtures(_FIXTURE_ROOT)
    atexit.register(shutil.rmtree, _TEMPORARY_ROOT, True)
    return _FIXTURE_ROOT, _WORK_ROOT, _FIXTURE_DIGEST


if __name__ == '__main__':
    main()
