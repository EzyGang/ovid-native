<h1 align="center">Ovid Native</h1>

<p align="center">
  Fast Rust-backed operations and typed tools for Ovid applications.
</p>

<p align="center">
  <a href="https://www.python.org/downloads/"><img src="https://img.shields.io/badge/Python-3.14%2B-blue" alt="Python 3.14 or newer"></a>
  <a href="https://www.rust-lang.org/"><img src="https://img.shields.io/badge/Rust-stable-orange" alt="Stable Rust"></a>
  <a href="pyproject.toml"><img src="https://img.shields.io/badge/version-0.1.0-indigo" alt="Version 0.1.0"></a>
  <a href="https://github.com/EzyGang/ovid-native/actions/workflows/ci.yml"><img src="https://github.com/EzyGang/ovid-native/actions/workflows/ci.yml/badge.svg" alt="CI status"></a>
</p>

---

**Ovid Native** gives Ovid applications fast workspace operations written in Rust. It includes safe file access, path and text search, warm indexed search, and AST search and rewrites.

The package also includes typed tools and capabilities for [Ovid Core](https://github.com/EzyGang/ovid-core). Installing it does not add tools to an agent. Your application selects each capability.

Every wheel contains the complete native surface. One package works for direct API calls and agent tools.

**Documentation:** [Ovid Native guide](https://github.com/EzyGang/ovid-core/tree/main/docs/content/native)  
**Source code:** https://github.com/EzyGang/ovid-native  
**Ovid Core:** https://github.com/EzyGang/ovid-core

---

## Table of contents

- [Why Ovid Native?](#why-ovid-native)
- [Installation](#installation)
- [Quick start](#quick-start)
- [Core features](#core-features)
- [Add tools to an agent](#add-tools-to-an-agent)
- [Safety and ownership](#safety-and-ownership)
- [Platforms](#platforms)
- [Development](#development)
- [Contributing](#contributing)

---

## Why Ovid Native?

| Need | What Ovid Native provides |
| --- | --- |
| Fast search | Rust path scanning, ripgrep search, and PCRE2 fallback |
| Warm repeated search | Long-lived FFF indexes for file and content lookup |
| Structural code work | ast-grep search and staged rewrites |
| Guarded file changes | Bounded reads, observations, writes, and patches |
| Typed Python API | Pydantic request and result models |
| Agent tools | Explicit Ovid capabilities with approval metadata |
| Replaceable providers | Ovid-owned protocols for files, search, AST, and views |

Rust owns native work, resources, cancellation, and platform behavior. Python owns validation, tool contracts, timeouts, approvals, and error conversion.

---

## Installation

Ovid Native requires Python 3.14 or newer.

### uv

```bash
uv add ovid-native
```

### pip

```bash
pip install ovid-native
```

You can record the parts used by your application:

```bash
uv add 'ovid-native[files,search,fff,ast]'
```

Use the full profile when the application needs every native integration:

```bash
uv add 'ovid-native[all]'
```

The profiles currently install the same wheel. They record the application contract and can add Python-only dependencies later.

---

## Quick start

Use an engine directly when application code needs native search:

```python
import asyncio
from pathlib import Path

from ovid_native.search import GlobRequest, SearchEngine


async def main() -> None:
    search = SearchEngine(root=Path('/workspace/project'))
    result = await search.glob(
        GlobRequest(
            patterns=('src/**/*.py', 'tests'),
            file_type='file',
            limit=200,
        )
    )

    for match in result.matches:
        print(match.path)


asyncio.run(main())
```

Search results are bounded and typed. Paths are relative to the workspace root.

Read the [workspace search guide](https://github.com/EzyGang/ovid-core/blob/main/docs/content/native/search.md) for text search, modes, limits, and pagination.

---

## Core features

### Safe workspace files

Read text with stable line observations. Create, replace, delete, move, and patch files through explicit policies. Changes reject stale observations and unsafe paths.

The file layer supports plain lines, hashline edits, and exact patches. It keeps byte order marks, line endings, and final newlines when required.

[Read the file guide](https://github.com/EzyGang/ovid-core/blob/main/docs/content/native/files.md)

### Workspace search

Find files with exact paths, directories, or glob patterns. Search UTF-8 content with literal, Rust regex, or PCRE2 patterns. Limits bound files, matches, bytes, context, and run time.

Ovid Native embeds the search libraries. It does not start an `rg` process.

[Read the search guide](https://github.com/EzyGang/ovid-core/blob/main/docs/content/native/search.md)

### Warm indexed FFF search

Keep one workspace index for repeated path and content searches. FFF supports fuzzy file lookup, indexed grep, and multi-pattern grep. Results state whether the index and result page are complete.

[Read the FFF guide](https://github.com/EzyGang/ovid-core/blob/main/docs/content/native/fff.md)

### AST search and rewrites

Use ast-grep patterns to find code by structure. Preview a rewrite before applying it. Apply checks the proposal, workspace revision, and source files again before it writes.

[Read the AST guide](https://github.com/EzyGang/ovid-core/blob/main/docs/content/native/ast.md)

---

## Add tools to an agent

Create one workspace service and add only the capabilities the agent needs:

```python
from pathlib import Path

from ovid_core.agents import AgentDefinition
from ovid_core.routing.models import ModelRef
from ovid_core.services import AgentServices
from ovid_native.ast import AstCapability
from ovid_native.search import SearchCapability
from ovid_native.workspace.service import NativeWorkspaceSession, workspace_binding


workspace = NativeWorkspaceSession(root=Path('/workspace/project'))

definition = AgentDefinition[None, str](
    model=ModelRef(name='primary'),
    deps_type=type(None),
    output_type=str,
    services=AgentServices((workspace_binding(workspace),)),
    capabilities=(
        SearchCapability(),
        AstCapability(),
    ),
)
```

Pass the definition to an Ovid Core `AgentFactory`. Call `await workspace.close()` when the agent lifetime ends.

Available capabilities include:

| Capability | Agent tools |
| --- | --- |
| `WorkspaceFilesCapability` | Bounded read, guarded write, and one selected edit mode |
| `SearchCapability` | `glob` and `grep` |
| `FffCapability` | `find_files`, indexed `grep`, `multi_grep`, and optional `glob` |
| `AstCapability` | AST search, rewrite preview, and rewrite apply |

Capabilities use the same workspace root, native handle, observations, and lifecycle. Missing services fail during agent construction.

---

## Safety and ownership

Ovid Native is trusted code inside the application process. It is not a sandbox.

The application owns:

- The workspace root.
- The enabled capabilities.
- File approval policy.
- Limits and timeouts.
- Workspace and index lifetime.
- User-facing errors and fallbacks.

Native operations reject paths outside the workspace. Long operations use cooperative cancellation. File changes use policy checks and current observations.

---

## Platforms

Published wheels target:

- Windows x86-64.
- Linux manylinux x86-64 and ARM64.
- macOS x86-64 and ARM64.

A source distribution is also supported. It includes the Rust source needed to build the extension.

---

## Development

Ovid Native uses uv, maturin, Cargo, Ruff, and ty.

Clone `ovid-native` next to `ovid-core`, then run:

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

`maturin develop` installs the local Rust extension. Run it again after changing a Rust export.

Public benchmarks are in [`benchmarks/`](benchmarks/README.md).

---

## Contributing

1. Open an issue for a new native operation or public contract.
2. Create a branch from `main`.
3. Update Rust, Python wrappers, type stubs, and tests together.
4. Run the Python and Rust checks.
5. Open a pull request with a clear reason for the change.

Keep the Python and Cargo versions equal. Do not edit generated extensions, build output, or `Cargo.lock` by hand. See [AGENTS.md](AGENTS.md) for the full repository rules.
