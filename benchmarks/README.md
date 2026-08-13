# Native Benchmarks

This suite measures the public `SearchEngine`, `AstEngine`, and `FffEngine` APIs. Timings include Python validation, PyO3 conversion, native work, and result mapping. They exclude fixture generation, FFF initial indexing, proposal setup for apply measurements, provider calls, agent construction, and model loops.

## Commands

Install the benchmark dependency and a development extension:

```bash
uv sync --group benchmark
uv run maturin develop
```

Run a short local check:

```bash
uv run task benchmark
uv run task benchmark -- --scenario grep.literal_sparse
```

CI smoke check:

```bash
uv run --group benchmark task benchmark-check
```

`.github/workflows/benchmarks.yml` runs this smoke set for benchmark and native search, AST, and FFF changes, and supports manual dispatch. It verifies execution only; shared-runner timings are not retained or compared.

Record a release result:

```bash
uv run task benchmark-record -- --record-version 0.1.0
uv run task benchmark-report
```

`benchmark-record` rejects a version that differs from the installed distribution, an unknown Git commit, a development profile, a dirty worktree, or an existing destination. `--allow-dirty` exists for investigation, not accepted release data. Results use `benchmarks/data/v<suite>/<package-version>/<machine-key>.json`. Never overwrite a record.

Compare equivalent records:

```bash
uv run task benchmark-compare -- \
  benchmarks/data/v1/0.1.0/<machine>.json \
  benchmarks/data/v1/0.2.0/<machine>.json
```

Comparison requires the same suite version, fixture version and digest, machine key, and build profile. A result is a regression only when pyperf's significance test reports a difference, the current median is at least 10% slower, and the absolute slowdown is at least 2 ms. The command exits nonzero on regression. `unstable` means the samples do not establish a significant change.

## Scenarios

Search fixtures contain 10,000 source files across 100 directories, deterministic modification times, sparse and dense tokens, Unicode text, one hot file, one oversized file, ignored and hidden files, a dependency subtree, and a binary file.

| ID | Measures |
| --- | --- |
| `glob.exact_file.10k` | Exact-file selection pruning |
| `glob.directory.10k` | Directory selection and subtree traversal |
| `glob.recursive_path.10k` | Recursive discovery and path ordering |
| `glob.recursive_mtime.10k` | Metadata retrieval and modified-time ranking |
| `grep.literal_miss.10k` | Complete literal-search miss |
| `grep.literal_sparse.10k` | Sparse literal matching and pagination |
| `grep.literal_dense.10k` | Dense file and match collection |
| `grep.pcre2_sparse.10k` | Embedded PCRE2 matching |
| `grep.hot_file.1mib` | Match collection and line indexing in one hot file |
| `grep.prefix_large_file` | Bounded prefix search of an oversized file |

Scaled search fixtures cover 100, 1,000, and 10,000 indexed files. Each FFF engine is long-lived and fully indexed before timing; pyperf repeats the same requests against that warm picker.

| ID | Measures |
| --- | --- |
| `glob.exact_file.small.100` | Native exact-path baseline over 100 files |
| `glob.exact_file.medium.1k` | Native exact-path baseline over 1,000 files |
| `glob.exact_file.large.10k` | Native exact-path baseline over 10,000 files |
| `grep.literal_sparse.small.100` | Native sparse literal baseline over 100 files |
| `grep.literal_sparse.medium.1k` | Native sparse literal baseline over 1,000 files |
| `grep.literal_sparse.large.10k` | Native sparse literal baseline over 10,000 files |
| `fff.find_typo.small.100` | Warm fuzzy path ranking over 100 indexed files |
| `fff.find_typo.medium.1k` | Warm fuzzy path ranking over 1,000 indexed files |
| `fff.find_typo.large.10k` | Warm fuzzy path ranking over 10,000 indexed files |
| `fff.grep_plain_sparse.small.100` | Warm indexed plain search over 100 files |
| `fff.grep_plain_sparse.medium.1k` | Warm indexed plain search over 1,000 files |
| `fff.grep_plain_sparse.large.10k` | Warm indexed plain search over 10,000 files |
| `fff.multi_grep_literal_or.large.10k` | Warm indexed literal OR search over 10,000 files |

AST fixtures contain 2,000 fixed Python files. Ten percent match the search and rewrite shape. A separate 100-file fixture supports apply measurements.

| ID | Measures |
| --- | --- |
| `ast.search_miss.2k` | Parsing and traversal without matches |
| `ast.search_sparse.2k` | Structural match construction without captures |
| `ast.search_captures.2k` | Capture extraction and Python mapping |
| `ast.preview_sparse.2k` | Rewrite matching, edit construction, and hashing |
| `ast.apply.100` | Applying 100 previously staged file changes |

## Reading Results

Use median wall time per public call for release comparisons. History tables also derive files per second from each scenario's fixed work-item count. Keep pyperf's individual values and warmups in the JSON record; they expose variance that a summary hides. Compare only fixed machine pools under similar thermal and power conditions. Cross-machine numbers are informational.

These are warm-filesystem, steady-state operation timings. Process startup, extension import, fixture construction, FFF initial indexing, and deliberate cold-cache manipulation are outside the contract. Add a separately named cold-start scenario only when application startup becomes a measured product requirement.

Shared CI runners are suitable for smoke execution, not release gates. A fixed self-hosted runner may upload raw JSON on every release candidate. A maintainer accepts the result only after reviewing variance and comparing it with the previous record from the same machine. Start automated gating after at least three accepted releases establish noise.

## Changing the Suite

A benchmark change is a contract change:

1. Add or modify the deterministic fixture in `benchmarks/fixtures.py`.
2. Add one scenario in `benchmarks/scenarios.py` with a stable ID, operation, and measured boundary.
3. Increment `FIXTURE_VERSION` when inputs change. Increment `SUITE_VERSION` when scenarios, limits, or timing boundaries change.
4. Update this scenario table and the benchmark guidance in `AGENTS.md`.
5. Run the focused scenario with `--fast`, then one rigorous release run on a fixed machine.
6. Do not compare records across suite or fixture versions.

Keep operations bounded and deterministic. Put setup and cleanup outside the returned timing. Do not add provider, agent-runtime, capability-discovery, cancellation, error-path, or metadata-enumeration timings to this suite. Add a Rust microbenchmark only after profiling identifies a specific native kernel that public API timing cannot isolate.
