import hashlib
import importlib.metadata
import json
import platform
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

from benchmarks.fixtures import FIXTURE_VERSION, SUITE_VERSION
from ovid_native.runtime import runtime_info


type JsonValue = None | bool | int | float | str | list[JsonValue] | dict[str, JsonValue]


DATA_ROOT = Path(__file__).parent / 'data'
RESULTS_PATH = Path(__file__).parent / 'results.md'
INDEX_PATH = DATA_ROOT / 'index.json'


@dataclass(frozen=True, slots=True)
class Environment:
    package_version: str
    git_commit: str
    git_dirty: bool
    build_profile: str
    system: str
    os_version: str
    architecture: str
    cpu_model: str
    python_implementation: str
    python_version: str
    python_compiler: str
    rustc_version: str
    machine_key: str

    def pyperf_metadata(self, fixture_digest: str) -> dict[str, int | str]:
        return {
            'ovid_suite_version': SUITE_VERSION,
            'ovid_fixture_version': FIXTURE_VERSION,
            'ovid_fixture_digest': fixture_digest,
            'ovid_package_version': self.package_version,
            'ovid_git_commit': self.git_commit,
            'ovid_git_dirty': int(self.git_dirty),
            'ovid_build_profile': self.build_profile,
            'ovid_native_api_version': runtime_info().api_version,
            'ovid_machine_key': self.machine_key,
            'ovid_cpu_model': self.cpu_model,
            'ovid_os_version': self.os_version,
            'ovid_python_compiler': self.python_compiler,
            'ovid_rustc_version': self.rustc_version,
        }


def environment(build_profile: str) -> Environment:
    package_version = importlib.metadata.version('ovid-native')
    git_commit = _command(['git', 'rev-parse', 'HEAD'], fallback='unknown')
    git_dirty = bool(_command(['git', 'status', '--porcelain'], fallback='dirty'))
    system = platform.system().casefold()
    os_version = platform.release()
    architecture = platform.machine().casefold()
    cpu_model = _cpu_model()
    python_implementation = platform.python_implementation().casefold()
    python_version = f'{sys.version_info.major}.{sys.version_info.minor}'
    python_compiler = platform.python_compiler()
    rustc_version = _command(['rustc', '--version'], fallback='unknown-rustc')
    fingerprint = '|'.join(
        (
            system,
            os_version,
            architecture,
            cpu_model,
            python_implementation,
            python_version,
            python_compiler,
            rustc_version,
        )
    )
    digest = hashlib.sha256(fingerprint.encode('utf-8')).hexdigest()[:12]
    prefix = '-'.join(
        (
            _slug(system),
            _slug(architecture),
            _slug(cpu_model)[:32],
            f'{_slug(python_implementation)}{sys.version_info.major}{sys.version_info.minor}',
        )
    )

    return Environment(
        package_version=package_version,
        git_commit=git_commit,
        git_dirty=git_dirty,
        build_profile=build_profile,
        system=system,
        os_version=os_version,
        architecture=architecture,
        cpu_model=cpu_model,
        python_implementation=python_implementation,
        python_version=python_version,
        python_compiler=python_compiler,
        rustc_version=rustc_version,
        machine_key=f'{prefix}-{digest}',
    )


def result_path(version: str, current: Environment) -> Path:
    return DATA_ROOT / f'v{SUITE_VERSION}' / version / f'{current.machine_key}.json'


def validate_recording(version: str, current: Environment, *, allow_dirty: bool) -> Path:
    if version != current.package_version:
        raise RuntimeError(f'Record version {version} does not match installed ovid-native {current.package_version}')
    if current.build_profile != 'release':
        raise RuntimeError('Recorded benchmarks require a release native extension')
    if current.git_commit == 'unknown':
        raise RuntimeError('Recorded benchmarks require an identifiable Git commit')
    if current.git_dirty and not allow_dirty:
        raise RuntimeError('Recorded benchmarks require a clean worktree; pass --allow-dirty for investigation')

    destination = result_path(version, current)
    if destination.exists():
        raise RuntimeError(f'Benchmark result already exists: {destination}')
    destination.parent.mkdir(parents=True, exist_ok=True)
    return destination


def sanitize_result(path: Path) -> None:
    document = json.loads(path.read_text(encoding='utf-8'))
    _sanitize(document)
    path.write_text(json.dumps(document, separators=(',', ':'), sort_keys=True) + '\n', encoding='utf-8')


def rebuild_index() -> None:
    records = sorted(str(path.relative_to(DATA_ROOT)) for path in DATA_ROOT.glob('v*/*/*.json') if path != INDEX_PATH)
    document = {'schema_version': 1, 'records': records}
    INDEX_PATH.parent.mkdir(parents=True, exist_ok=True)
    INDEX_PATH.write_text(json.dumps(document, indent=2, sort_keys=True) + '\n', encoding='utf-8')


def comparison_key(metadata: dict[str, JsonValue]) -> tuple[JsonValue | None, ...]:
    keys = (
        'ovid_suite_version',
        'ovid_fixture_version',
        'ovid_fixture_digest',
        'ovid_machine_key',
        'ovid_build_profile',
    )
    return tuple(metadata.get(key) for key in keys)


def _sanitize(value: JsonValue) -> None:
    if isinstance(value, dict):
        for key in ('hostname', 'python_executable', 'command'):
            value.pop(key, None)
        for nested in value.values():
            _sanitize(nested)
    elif isinstance(value, list):
        for nested in value:
            _sanitize(nested)


def _command(arguments: list[str], *, fallback: str) -> str:
    try:
        completed = subprocess.run(
            arguments,
            check=True,
            capture_output=True,
            text=True,
        )
    except OSError, subprocess.CalledProcessError:
        return fallback

    return completed.stdout.strip()


def _cpu_model() -> str:
    if sys.platform == 'darwin':
        model = _command(['sysctl', '-n', 'machdep.cpu.brand_string'], fallback='')
        if model:
            return model

    return platform.processor() or platform.machine() or 'unknown-cpu'


def _slug(value: str) -> str:
    normalized = re.sub(r'[^a-z0-9]+', '-', value.casefold()).strip('-')
    return normalized or 'unknown'
