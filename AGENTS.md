# Repository Guidelines

## Project

`ovid-native` is the native implementation package for Ovid applications. One PyO3 extension contains every supported Rust operation. Typed Python models, tools, toolsets, and capabilities connect those operations to `ovid-core`.

Keep this as one distribution. Do not split search, files, AST, process, terminal, desktop, media, or future native operations into separate Python packages. Every wheel must expose the same supported native surface.

`ovid-native` is optional. `ovid-core` never depends on it. Installing this package must not add tools to an agent. Applications choose each capability explicitly.

## Architecture and dependency rules

Use this dependency direction:

```text
Ovid application
      |
      v
ovid_native Python API ------> ovid_core public contracts
      |
      v
private PyO3 module
      |
      v
Rust implementation
```

Place code according to ownership:

- Put algorithms, native resources, platform behavior, concurrency, and cancellation in Rust.
- Put validation, public models, tool contracts, approvals, timeouts, and error translation in Python.
- Put prompts, permissions, tool selection, rendering, config discovery, and fallback policy in the application.
- Never import `pydantic_ai` or provider SDK runtime types from `ovid-native`.
- Never add an `ovid-native` dependency to `ovid-core`.

Keep the PyO3 boundary small. Pass validated scalars or purpose-built native values. Return typed native results. Do not send JSON, Pydantic models, agent contexts, or application services through the boundary.

## Folder structure

Mirror each domain across Rust, Python, tests, and documentation:

```text
src/
├── <domain>/                         Rust algorithms and PyO3 exports
└── lib.rs                            Private extension module
python/ovid_native/
├── <domain>/                         Models, engines, tools, and capabilities
├── _native.pyi                       Type declarations for Rust exports
├── _native_execution.py              Shared boundary execution
└── __init__.py                       Empty by design
tests/
└── test_<domain>_*.py                Python and installed-extension contracts
benchmarks/                           Public end-to-end performance contracts
../ovid-core/docs/content/native/     User guides
```

A search domain change should look like this:

```text
src/search/                           Native search implementation
python/ovid_native/search/
├── models.py
├── engine.py
├── tools.py
├── capability.py
└── errors.py
tests/test_search_*.py
../ovid-core/docs/content/native/search.md
```

Organize code by domain, not by language-level type or release stage. Do not create generic `schemas`, `dto`, `interfaces`, `utils`, or `helpers` packages. Add shared code only after more than one domain needs it.

Keep cross-domain boundary code at the nearest shared package level. `python/ovid_native/_native_execution.py` is one example.

Generated extensions, Python caches, `target/`, `dist/`, coverage output, and documentation site output are build state. Do not edit or commit them.

## Package and version rules

- The distribution is `ovid-native`.
- The import package is `ovid_native`.
- The private extension is `ovid_native._native`.
- Keep the versions in `pyproject.toml` and `Cargo.toml` equal.
- `Cargo.lock` is generated and committed. Update it with Cargo; never edit it by hand.
- Follow semantic versioning independently from `ovid-core`.
- Declare the supported `ovid-core` range in `pyproject.toml`.
- Increment the native API version from `_native.runtime_info()` when Python and Rust become incompatible.

## Development commands

Run commands from the repository root:

```bash
uv sync
uv run maturin develop
uv run task ruff
uv run task ty-lint
uv run task rust-lint
uv run task rust-tests
uv run task tests
uv run task build
```

Run `uv run maturin develop` after changing Rust exports and before Python tests that import `_native`.

The source distribution is supported. It must include all Rust source and workspace dependencies needed to build outside the repository checkout.

## Python rules

### Types and models

- Target Python 3.14. Use current generic syntax instead of `TypeVar` or `ParamSpec` declarations.
- Annotate every parameter and return value. Parameterize every collection.
- Use a precise type when possible. Use `Any` when the value is truly dynamic; do not use `object` as a substitute.
- Do not silence ty. Fix the type model or report the blocker.
- Public structured DTOs inherit `ovid_core.models.BaseModel`. UUID-like root values inherit `BaseRootModel`.
- Use Pydantic models for validated data. Use plain classes for engines, native handles, factories, routers, and stores.
- Return typed models for complex results. Do not return unstructured dictionaries or lists of dictionaries.

### Naming and imports

- Use `snake_case` for modules, functions, and variables. Use `PascalCase` for types.
- Ruff controls formatting: 120-column lines, four spaces, single quotes, and sorted imports.
- Use keyword arguments when a call passes several values.
- Use f-strings for interpolation. Do not use `.format()`, `%` formatting, or string concatenation for templates.
- Avoid local imports. Break import cycles by changing module boundaries first.
- Keep `python/ovid_native/__init__.py` empty. Consumers import from the domain that owns a symbol.
- Do not define `__all__` or add package-level re-exports.

### Boundary and runtime behavior

- Map validated fields directly into PyO3 calls. Avoid `model_dump()`, JSON, and temporary dictionaries when typed arguments are enough.
- Imports may load the extension but must not start threads, inspect workspaces, read credentials, spawn processes, or activate tools.
- Use async for filesystem, process, terminal, network, and tool I/O. Do not block the event loop.
- Raise narrow exceptions and preserve causes. Never include secrets, credentials, signed URLs, or sensitive file content in errors.
- Keep functions within 40 lines and files within 250 lines unless the behavior cannot be split cleanly.
- Use guard clauses and shallow indentation.
- Separate setup, decisions, side effects, and return values with blank lines.
- Add an abstraction only when it removes real duplication or isolates the Python/Rust boundary.
- Add comments or docstrings only when a critical rule is not clear from names and types.

## Rust rules

Target stable Rust with edition 2024. `rustfmt` and Clippy control formatting and linting. Unsafe Rust is forbidden unless a reviewed native operation cannot use a safe abstraction.

### Types and errors

- Put trait bounds in `where` clauses.
- Use conventional generic names: `S` for streams, `Fut` for futures, and `F` for functions.
- Add generic conversion only when callers need more than one input type.
- Derive `Copy`, `Clone`, `Eq`, `PartialEq`, `Hash`, and `Debug` for public value types when fields allow it.
- Derive `Default` only when the default has a clear meaning.
- Mark side-effect-free result functions `#[must_use]` when ignoring the value is almost always a bug.
- Prefer `Self` inside an implementation.
- Return narrow errors for expected failures. Do not panic for input, platform, cancellation, filesystem, process, or parse errors.
- Never use `unwrap`, `expect`, `todo!`, `unimplemented!`, or placeholder success values in runtime code.

### Control flow and modules

- Use `.to_owned()` to convert `&str` to `String`.
- Prefer `Box::pin(async { ... })` to `.boxed()`.
- Use early returns for failure or absence.
- Use `match` when both variants have behavior. Use `=> (),` for an intentionally empty arm.
- Use full logging macro paths such as `tracing::debug!`.

Order imports in separate groups: `std`, external crates, `crate`, local modules, then sparse re-exports. Use one `use` declaration per crate.

Order file contents for a first-time reader:

1. public types
2. public functions
3. private types
4. implementations and private helpers in call order

Documentation comments state API guarantees. Do not explain private code that is already clear from its name and type.

## PyO3 boundary rules

- Use maturin as the only Python build backend.
- Keep the extension private as `ovid_native._native`.
- Update `_native.pyi` with every Rust export change.
- Keep the normal wheel compatible with `abi3-py314`.
- Release the GIL for blocking or CPU-bound work.
- Do not keep borrowed Python values across an await point.
- Long operations receive and check cooperative cancellation state.
- Cross the boundary once per result or bounded stream item, not once per byte or AST node.
- Do not expose pointers, Rust lifetimes, Tokio handles, Pydantic AI values, or provider SDK objects.
- Start runtimes and worker pools lazily from explicit operations, not during module loading.
- Native code is trusted in-process code, not a sandbox. Python tool wrappers must keep Ovid approval and policy checks.

Free-threaded CPython support is explicit. Do not mark the module GIL-independent until every export and native handle is thread-safe and tested on Python 3.14t.

## Capabilities and platform behavior

- Each operation is callable through its owning Python domain.
- Group related operations in a `BaseToolset` only when the group has a clear shared lifecycle.
- Capabilities add tools only when the application passes them to `AgentFactory`.
- Namespace stable capability and tool IDs under `ovid_native`.
- Treat duplicate IDs and implicit overrides as errors.
- Keep Rust operations domain-neutral. Agent roles, prompts, repository policy, reviews, and UI belong to the application.
- Report unsupported platforms with typed errors. Never use a no-op or unrelated Python fallback.

Publish standard wheels and one source distribution. Do not add an import-time downloader or custom binary cache.

Supported wheel families are Windows x86-64, Linux manylinux x86-64 and ARM64, and macOS x86-64 and ARM64. Add another platform only for a supported consumer.

Use a conservative CPU baseline and runtime feature detection. Prefer pure-Rust dependencies. A system-library dependency needs a wheel portability plan before it lands.

Do not publish different native APIs behind Cargo feature combinations for the same package version.

## Benchmark rules

Public benchmarks live in `benchmarks/`. Measure through `SearchEngine`, `AstEngine`, and `FffEngine` so results include validation, PyO3 conversion, native work, and result mapping.

- Use deterministic generated fixtures.
- Do not benchmark the checkout, network services, provider calls, agent loops, or clock-dependent data.
- Finish FFF indexing before timing warm queries.
- Keep scenario IDs stable.
- Increment `FIXTURE_VERSION` when fixtures change and `SUITE_VERSION` when scenarios or measured limits change.
- Keep fixture setup, rewrite preparation, restoration, and cleanup outside timed sections.
- Never overwrite a raw result.
- Compare only records with the same suite, fixture, machine, Python, and build profile.
- Treat a result as a regression only when it is statistically significant, at least 10% slower, and at least 2 ms slower.

Use these commands:

```bash
uv run task benchmark -- --scenario <name>
uv run task benchmark-record -- --record-version <version>
uv run task benchmark-report
```

## Testing and validation

For each native operation, test:

- Rust success and failure behavior
- the public Python wrapper against the installed extension
- cancellation and cleanup for long or resource-owning work
- unsupported-platform behavior when relevant
- one smoke path through the public Python API

Use `mocker: MockerFixture` for every Python double, patch, spy, or environment change. Do not use `unittest.mock`, `monkeypatch`, or another mocking helper.

Before reporting work complete, run:

```bash
uv run task ty-lint
uv run ruff format --check ./python/ovid_native ./tests ./benchmarks
uv run ruff check ./python/ovid_native ./tests ./benchmarks
uv run task rust-lint
uv run task rust-tests
uv run maturin develop
uv run task tests
uv run task build
```

The Python integration layer requires 100% branch coverage. Install the built wheel in a clean environment and exercise a changed operation. For release changes, also build and install the source distribution in isolation.

Do not weaken checks or thresholds. If a repository problem blocks a check, report the exact blocker.
