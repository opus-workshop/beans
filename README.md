# beans

A hierarchical task engine where every task is a YAML file.

No databases. No daemons. No background processes. Just files you can read, edit, grep, and git-diff.

> **Status:** Design complete. Not yet implemented. See [`.beans/bean.yaml`](.beans/bean.yaml) for the full spec.

## Origin

Beans is inspired by Steve Yegge's [beads](https://github.com/steveyegge/beads) — a distributed, git-backed issue tracker built for AI coding agents. Beads proved that structured task management with dependency graphs and readiness checks is the right foundation for agent-driven development. If you haven't looked at it, you should.

We wanted a task engine for coding subagents and beads was the obvious starting point. But as we worked with it, we found ourselves wanting different tradeoffs — not because beads got something wrong, but because we wanted to build around a different center of gravity.

Beans takes beads' core idea and applies Unix philosophy: **everything is a file**. No database, no daemon, no CLI as the only interface. YAML files you can open in your editor, grep across, git-diff, and compose with standard tools. Parseable IDs, plain text, stateless operations. Beads is a closed system that does everything. Beans is a file format the Unix ecosystem can talk to.

## Why

Beans is a tool for goal-driven development. You start with a goal, decompose it into smaller goals, and keep splitting until every leaf is an agent-executable unit of work with a verification command that proves it's done. The tool enforces this: `bn close` runs the bean's `verify` command and only closes if it passes. No force flag. If the test fails, the work isn't done.

A bean is a unit of work with enough context to execute autonomously. Parent beans provide strategic context. Leaf beans are self-contained agent prompts — swarmable. The hierarchy lives in the filesystem: `3.2.yaml` is a child of `3.yaml`. You see the structure before opening a single file.

Git is the sync layer. `git add .beans/ && git commit` is all there is.

## Quick Start

```
bn init my-project
bn create "Build authentication system"
bn create "Design token schema"
bn create --deps 2 "Implement token validation"
bn ready
```

## How It Works

### The `.beans/` directory

```
.beans/
  config.yaml          # project settings
  bean.yaml            # root goal — the strategic "what and why"
  index.yaml           # auto-rebuilt cache (never edit manually)
  1.yaml               # "Build authentication system"
  2.yaml               # "Design token schema"
  3.yaml               # "Implement CLI"
  3.1.yaml             # "bn create command" (child of 3)
  3.2.yaml             # "bn list/show commands" (child of 3)
```

Every numbered file is a bean. IDs are sequential integers with dot-notation for children. The index is a cache — YAML files are the source of truth.

### A bean looks like this

```yaml
id: 3.2
title: Implement bn list and bn show commands
status: open
priority: 2
parent: 3
dependencies:
  - 2
labels:
  - cli
  - core
created_at: 2026-01-26T15:00:00-08:00
description: |
  Implement the `bn list` and `bn show` commands.

  ## bn show <id>
  - Read .beans/{id}.yaml and display all fields
  - Support --json flag for machine-readable output

  ## bn list [flags]
  - Read from index.yaml (rebuild if stale)
  - Filter by: --status, --priority, --parent, --label
  - Default: tree-format output with status indicators

  ## Files
  - src/commands/show.rs
  - src/commands/list.rs

acceptance: |
  - `bn show 1` displays the YAML for bean 1
  - `bn show 1 --json` outputs valid JSON
  - `bn list` shows tree-format with status indicators
  - `bn list --parent 3` shows only children of bean 3

verify: cargo test --lib commands::list commands::show
```

The description is the agent prompt. Rich enough that an agent can pick it up cold and execute. The `verify` field is the machine-checkable gate — `bn close` runs it and only closes the bean if it exits 0.

## Commands

### Core

| Command | Description |
|---|---|
| `bn init [name]` | Initialize `.beans/` in the current directory |
| `bn create [title]` | Create a new bean |
| `bn show <id>` | Display a bean (raw YAML, `--json`, or `--short`) |
| `bn list` | List beans with filtering (`--status`, `--priority`, `--tree`) |
| `bn update <id>` | Modify bean fields |
| `bn close <id>` | Run verify command; close only if it passes |
| `bn verify <id>` | Run verify command without closing |
| `bn reopen <id>` | Reopen a closed bean |
| `bn delete <id>` | Remove a bean and clean up references |

### Dependencies

| Command | Description |
|---|---|
| `bn dep add <id> <dep>` | Add a dependency (id waits for dep) |
| `bn dep remove <id> <dep>` | Remove a dependency |
| `bn dep list <id>` | Show dependencies and dependents |
| `bn dep tree [id]` | Dependency tree with box-drawing characters |
| `bn dep cycles` | Detect and report cycles |

### Views

| Command | Description |
|---|---|
| `bn ready` | Beans with no blocking dependencies, sorted by priority |
| `bn blocked` | Beans waiting on unresolved dependencies |
| `bn tree [id]` | Hierarchical tree with status indicators |
| `bn graph` | Dependency graph (`--format mermaid` or `dot`) |
| `bn stats` | Counts, priority breakdown, completion progress |
| `bn doctor` | Health check — orphans, cycles, index freshness |
| `bn sync` | Force index rebuild |

### Example Output

```
bn list --tree

[ ] 1. Project scaffolding
[ ] 2. YAML data model
[-] 3. Implement CLI
  [x] 3.1 bn create command
  [-] 3.2 bn list/show commands
  [ ] 3.3 bn dep command
[ ] 4. Implement auto-index

Legend: [ ] open  [-] in_progress  [x] closed  [!] blocked
```

```
bn ready

P0  1    Project scaffolding
P0  2    YAML data model
P2  3.3  bn dep command
```

## The Planning Workflow

Beans is designed for progressive decomposition:

1. **Create a goal** — `bn create "Build the thing"`
2. **Decompose** — split into sub-beans until leaves are agent-executable
3. **Check readiness** — each leaf should be self-contained, bounded, testable, unambiguous, and fit in context
4. **Swarm** — dispatch leaf beans to agents in dependency-aware waves
5. **Close** — agents write handoff notes, mark beans closed, dependents become ready

A leaf bean is ready for autonomous execution when:

- **Self-contained** — enough context to start without asking questions
- **Bounded** — 1-5 functions to write, 2-10 to read
- **Testable** — concrete acceptance criteria, not "works correctly"
- **Unambiguous** — the "how" is clear, not just the "what"
- **Fits in context** — estimated <64k tokens

## Beans vs Beads

The two tools share the same core idea — dependency-aware task graphs for autonomous agents — but make different tradeoffs at every layer.

### Where beans wins

**Direct file access.** An agent can `cat .beans/3.2.yaml` and have everything it needs. No CLI dependency, no database query, no daemon running. The data is the interface. With beads, the CLI is a required intermediary between the agent and the task.

**Parseable IDs.** An agent sees `3.2` and knows it's a child of `3` without a lookup. `ls .beans/` reveals the full hierarchy. Hash IDs require querying to understand relationships.

**No hidden state.** One file = one source of truth. No JSONL + SQLite cache + daemon sync where things can diverge. An agent reads the file and it's guaranteed current.

**Self-contained format.** A bean's description field is the agent prompt — same file that stores the metadata also carries the full execution context. No extraction step, no joining across records.

**Universal tooling.** `grep -r "authentication" .beans/` searches every bean. `git diff .beans/` shows what changed. Any tool that works with files works with beans. No special CLI required.

### Where beads wins

**Merge-safe IDs.** Hash-based IDs never collide. Two agents creating beans on separate branches will produce duplicate sequential IDs. Beads handles distributed creation natively; beans currently requires coordination.

**Query performance at scale.** SQLite handles thousands of issues with indexed queries. Beans rebuilds a flat YAML index by scanning files — fine for hundreds, potentially slow for thousands.

**Automatic sync.** Beads' background daemon keeps the local cache fresh without explicit commands. Beans is stateless — faster to reason about, but you manage freshness yourself.

### Summary

Beads optimizes for distributed infrastructure at scale. Beans optimizes for directness — and directness matters to agents and humans equally. An agent that can read a file without booting a daemon, parse an ID without a lookup, and grep across tasks without a query language has fewer moving parts between it and the work.

The tradeoff is real: beans currently lacks a story for multi-branch parallel creation without ID conflicts. That's a solvable problem (see [Future Work](#future-work)), not a fundamental limitation.

## Design Decisions

**YAML files, not a database.** Each bean is a file you can open in your editor, browse in a file tree, grep across, and git-diff. The filesystem is the query interface.

**Sequential IDs with dot-notation.** `ls .beans/` shows structure. `3.2.yaml` is obviously a child of `3.yaml`.

**No fixed types.** Has children = container. No children = leaf. A "task" that gets decomposed becomes a "subgoal" implicitly. Types add ceremony without value.

**Stateless CLI.** Read files, write files, exit. No daemons, no lock files, no PID files. Index staleness is checked via mtime comparison. The index is a cache, not a source of truth — eliminates an entire class of sync bugs.

**Built from scratch, not forked.** Beads is built around JSONL + SQLite + a background daemon. Beans wants individual YAML files, sequential IDs, and a stateless CLI. That's not a refactor — it's a different architecture. Forking would mean gutting everything except the dependency graph logic. Starting fresh is cleaner when the overlap is conceptual rather than structural.

## Tech Stack

- **Language:** Rust
- **CLI:** Clap 4 (derive macros)
- **Serialization:** Serde + serde_yaml
- **Error handling:** Anyhow
- **Timestamps:** Chrono

## Future Work

**Branch-safe ID allocation.** Sequential IDs can collide when agents create beans on parallel branches. Possible approaches: ID reservation ranges per branch, a lightweight lock file, or a rebase-time renumbering pass. The goal is to solve this without giving up readable IDs or requiring a daemon.

**Scaling the index.** The current design scans all YAML files to rebuild the index. For projects with thousands of beans, this could benefit from incremental indexing — updating only changed files rather than full rebuilds.

## License

MIT
