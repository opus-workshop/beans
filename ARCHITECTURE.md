# Architecture

> Last updated: 2026-02-23
> Manual edits welcome — recon preserves them and flags drift.

## Overview

**beans** (`bn`) is a task tracker designed for AI coding agents. Each task ("bean") has a verify gate — a shell command that must exit 0 to close. This enforces fail-first TDD: the verify command must fail before implementation, then pass after. No databases, no daemons — just `.beans/` markdown files you can `cat`, `grep`, and `git diff`.

One-sentence: **Markdown task files with dependency graphs and verification gates, orchestrated for parallel AI agents.**

## Tech Stack

- **Language:** Rust (edition 2021)
- **CLI framework:** clap 4 (derive macros)
- **Serialization:** serde + serde_json + serde_yaml (⚠️ serde_yaml 0.9 is deprecated)
- **Error handling:** anyhow (Result + bail! + .context() throughout)
- **Terminal UI:** termimad (markdown rendering), dialoguer (interactive prompts)
- **Hashing:** sha2 (for content checksums)
- **Time:** chrono with serde feature
- **Storage:** Plain files in `.beans/` directory — YAML index + markdown bean files

## System Context

```
┌─────────────┐     ┌──────────────┐     ┌─────────────┐
│  Developer   │────▶│   bn CLI     │────▶│  .beans/    │
│  or Agent    │     │              │     │  (files)    │
└─────────────┘     └──────┬───────┘     └─────────────┘
                           │
                    ┌──────┴───────┐
                    │  bn run      │──── spawns ────▶ Agent processes (pi, claude, etc.)
                    └──────────────┘
                           │
                    ┌──────┴───────┐
                    │  MCP server  │◀─── stdio ────── IDE (Cursor, Claude Desktop, etc.)
                    └──────────────┘
```

**External systems this touches:**
- **git** — worktree operations for agent sandboxing, change history
- **Shell** — verify commands, agent process spawning
- **pi / claude CLI** — agent processes spawned by `bn run`
- **IDE MCP clients** — Cursor, Claude Desktop, Windsurf via JSON-RPC 2.0 over stdio

## Building Blocks

```
src/
├── main.rs              — CLI entry point, command dispatch
├── cli.rs               — clap definitions (35+ subcommands)
├── lib.rs               — Module declarations (20 modules)
├── bean.rs              — Core Bean type, Status, RunRecord, verification history
├── index.rs             — Index (YAML cache of all bean metadata)
├── config.rs            — Config (.beans/config.yaml parsing, inheritance via extends)
├── discovery.rs         — Find .beans/ dir, locate bean files by ID
├── graph.rs             — Dependency graph, cycle detection, topological sort
├── commands/
│   ├── mod.rs           — Command module declarations
│   ├── create.rs        — Bean creation with slug generation
│   ├── close.rs         — Verification + close logic (largest command, 2737L)
│   ├── run.rs           — Agent orchestration: waves, dispatch, monitoring
│   ├── plan.rs          — Bean decomposition planning
│   ├── show.rs          — Bean display with markdown rendering
│   ├── edit.rs          — Interactive bean editing ($EDITOR)
│   ├── update.rs        — Field-level bean updates
│   ├── init.rs          — Project initialization with agent presets
│   ├── list.rs          — Filtered listing with status/label/assignee
│   ├── claim.rs         — Bean claiming (locks for agents)
│   ├── quick.rs         — Create + claim in one step
│   ├── context.rs       — Assemble file context from bean descriptions
│   ├── agents.rs        — Monitor running agents
│   ├── status.rs        — Project overview (claimed/ready/blocked)
│   ├── ready.rs         — Show unblocked beans
│   ├── verify.rs        — Run verify without closing
│   ├── dep.rs           — Dependency management (add/remove/list)
│   ├── tidy.rs          — Clean up stale data
│   ├── tree.rs          — Hierarchical bean display
│   ├── graph.rs         — DOT/Mermaid dependency visualization
│   ├── logs.rs          — Agent log viewer
│   ├── doctor.rs        — Health checks
│   ├── sync.rs          — Index rebuild from files
│   ├── adopt.rs         — Reparent beans
│   ├── trust.rs         — Trust management for verify commands
│   ├── recall.rs        — Memory recall
│   ├── memory_context.rs — Memory context assembly
│   ├── fact.rs          — Fact storage
│   ├── stats.rs         — Project statistics
│   ├── config_cmd.rs    — Config CLI (get/set)
│   ├── reopen.rs        — Reopen closed beans
│   ├── delete.rs        — Bean deletion with cleanup
│   ├── unarchive.rs     — Restore archived beans
│   ├── stdin.rs         — Pipe input handling
│   └── interactive.rs   — Interactive bean creation (dialoguer)
├── mcp/
│   ├── mod.rs           — MCP module
│   ├── server.rs        — JSON-RPC 2.0 stdio server loop
│   ├── protocol.rs      — Request/response types
│   ├── tools.rs         — Tool definitions (create, close, list, etc.)
│   └── resources.rs     — Resource definitions (bean content)
├── spawner.rs           — Agent process lifecycle (spawn, track, log, cleanup)
├── stream.rs            — JSON streaming events for bn run --json-stream
├── pi_output.rs         — Parse pi agent output (events, tokens, costs)
├── ctx_assembler.rs     — Extract file paths from descriptions, assemble context
├── relevance.rs         — File relevance scoring for context assembly
├── hooks.rs             — Post-close and on-fail hook execution
├── agent_presets.rs     — Detect and configure agents (pi, claude, aider, etc.)
├── worktree.rs          — Git worktree isolation for parallel agents
├── timeout.rs           — Agent timeout monitoring
├── tokens.rs            — Token counting for context budgets
├── project.rs           — Project type detection (Rust, Node, Python, etc.)
└── util.rs              — Shared utilities (ID validation, natural sort, slugs)

tests/
├── cli_tests.rs         — Integration tests (5 test functions)
├── test_ctx_assembler.rs — Context assembler unit tests (22 tests)
└── adopt_test.rs        — Adopt command tests (10 tests)

docs/
├── SKILL.md             — Agent skill definition for beans
├── BEST_PRACTICES.md    — Guide for creating effective beans
├── fail-then-pass-design.md — Design doc for fail-first verification
└── design/
    └── CONFLICT_RESOLUTION.md — Design doc for merge conflicts
```

### Internal Dependency Flow

```
main.rs ──▶ cli.rs (parse) ──▶ commands/*.rs (execute)
                                     │
                                     ▼
                              ┌─────────────┐
                              │  bean.rs     │ ◀── Core types
                              │  index.rs    │ ◀── Metadata cache
                              │  config.rs   │ ◀── Project settings
                              │  discovery.rs│ ◀── File location
                              └─────────────┘
                                     │
                              ┌──────┴──────┐
                              │  graph.rs   │ ◀── Dependency resolution
                              │  spawner.rs │ ◀── Agent process management
                              │  hooks.rs   │ ◀── Event-driven actions
                              │  worktree.rs│ ◀── Git isolation
                              └─────────────┘
```

**Load-bearing modules** (high fan-in — most commands import these):
- `bean.rs` — Bean, Status
- `index.rs` — Index, IndexEntry
- `discovery.rs` — find_beans_dir, find_bean_file
- `config.rs` — Config
- `util.rs` — validate_bean_id, natural_cmp, title_to_slug

## Data Model

**No database.** All state lives in `.beans/` directory as plain files.

| File | Format | Purpose |
|------|--------|---------|
| `.beans/config.yaml` | YAML | Project settings, agent templates, inheritance |
| `.beans/index.yaml` | YAML | Fast lookup cache of all bean metadata |
| `.beans/{id}-{slug}.md` | Markdown with YAML frontmatter | Individual bean definitions |
| `.beans/archive/` | Same as above | Closed/archived beans |

**Bean file structure** (markdown format):
- YAML frontmatter: id, title, status, priority, parent, dependencies, verify, produces, requires, labels, assignee, claimed_by, attempts, on_fail, on_close, created_at, updated_at
- Markdown body: description, acceptance criteria, context

**Index is a cache** — can be rebuilt from bean files via `bn sync`.

**Key relationships:**
- Beans form a tree (parent/child via `parent` field)
- Beans form a DAG (dependencies via `dependencies` field)
- Beans can declare `produces`/`requires` for artifact-based dependency inference

## Development

### Prerequisites
- Rust toolchain (edition 2021)
- git (for worktree features)

### Commands
| Action | Command |
|--------|---------|
| Build | `cargo build` |
| Build release | `cargo build --release` |
| Test | `cargo test` |
| Install from source | `cargo install --path .` |
| Install from git | `cargo install --git https://github.com/opus-workshop/beans` |

### No CI/CD configured
No GitHub Actions, Makefile, or Justfile. Tests run locally only.

💡 Consider adding a GitHub Actions workflow for CI.

## Conventions & Patterns

- **Error handling:** `anyhow::Result` everywhere, `.context()` for error chains, `bail!` for early returns
- **CLI structure:** One file per command in `src/commands/`, each exports a `cmd_*` function
- **Serialization:** serde derive on all types, `#[serde(skip_serializing_if)]` for optional fields
- **File naming:** Bean files use `{id}-{slug}.md` format (legacy: `{id}.yaml`)
- **ID validation:** All bean IDs validated via `util::validate_bean_id()` to prevent path traversal
- **Sorting:** Natural sort (`util::natural_cmp`) for bean IDs (1, 2, 10 not 1, 10, 2)
- **Testing:** Heavy inline `#[cfg(test)]` modules — 851 tests, mostly unit tests inside source files
- **No async:** Entire codebase is synchronous (no tokio/async-std)
- **Module exports:** `lib.rs` re-exports all modules as `pub mod`, commands behind `commands::mod.rs`

## Health & Risks

### Hotspots (churn × size)

| Score | Churn | Size | File | Notes |
|-------|-------|------|------|-------|
| 60,214 | 22× | 2,737L | `commands/close.rs` | Largest command — verification + fail-first + hooks |
| 56,753 | 29× | 1,957L | `commands/create.rs` | Most-changed command |
| 32,460 | 20× | 1,623L | `bean.rs` | Core type — changes ripple everywhere |
| 27,520 | 40× | 688L | `main.rs` | High churn from command dispatch growth |
| 26,336 | 32× | 823L | `cli.rs` | Grows with every new subcommand |

### Temporal Coupling (files that change together)

| Co-commits | Pair | Why |
|-----------|------|-----|
| 15 | `cli.rs` ↔ `main.rs` | Every new command touches both |
| 9 | `commands/mod.rs` ↔ `main.rs` | Command registration |
| 7 | `cli.rs` ↔ `commands/mod.rs` | Command definition chain |

This is expected — adding a command requires cli.rs (args) + mod.rs (module) + main.rs (dispatch).

### Test Coverage

- **851 tests** across source files and 3 test files
- Heavy inline testing (`#[cfg(test)]` modules in most source files)
- `close.rs` has the most tests (75) — appropriate given its complexity
- `ctx_assembler.rs` (49 inline + 22 in test file) and `bean.rs` (53) well tested
- `util.rs` has 51 tests — good coverage of shared utilities

### Notable Gaps
- **No CI** — tests only run locally
- **serde_yaml 0.9 is deprecated** — should migrate to a maintained YAML library
- **No cargo-audit** — security vulnerabilities not checked
- **commands/run.rs** (1,663L) is large and growing — orchestration logic may warrant extraction

### In-Progress Work (from .beans/)
- Bean 11: Verify-on-claim (run verify before granting claim)
- Bean 12: Git worktree sandboxing for parallel agents
- Bean 75: Project rules (RULES.md convention)
- Bean 76: MCP server
- Beans 77-80: Various features (hooks, worktree isolation, gitignore, identity)
