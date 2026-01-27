# beans

A hierarchical task engine where every task is a YAML file.

No databases. No daemons. No background processes. Just files you can read, edit, grep, and git-diff.

## Origin

Beans is inspired by Steve Yegge's [beads](https://github.com/steveyegge/beads) — a distributed, git-backed issue tracker built for AI coding agents. Beads proved that structured task management with dependency graphs and readiness checks is the right foundation for agent-driven development. If you haven't looked at it, you should.

Beans takes that foundation and rebuilds it around a different set of tradeoffs: individual YAML files instead of JSONL, sequential IDs instead of hashes, a stateless CLI instead of daemons and SQLite caches. Where beads optimizes for distributed multi-agent infrastructure, beans optimizes for a human staring at a file tree.

The core insight is the same. The interface is different.

## Why

A bean is a unit of work with enough context to execute autonomously. Parent beans provide strategic context. Leaf beans are self-contained agent prompts — swarmable. The hierarchy lives in the filesystem: `3.2.yaml` is a child of `3.yaml`. You see the structure before opening a single file.

Git is the sync layer. `git add .beans/ && git commit` is all there is.

## Quick Start

```
bn init my-project
bn create "Build authentication system"
bn create --parent 1 "Design token schema"
bn create --parent 1 --deps 2 "Implement token validation"
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
```

The description is the agent prompt. Rich enough that an agent can pick it up cold and execute. File paths, code snippets, design decisions — all inline.

## Commands

### Core

| Command | Description |
|---|---|
| `bn init [name]` | Initialize `.beans/` in the current directory |
| `bn create [title]` | Create a new bean |
| `bn show <id>` | Display a bean (raw YAML, `--json`, or `--short`) |
| `bn list` | List beans with filtering (`--status`, `--priority`, `--tree`) |
| `bn update <id>` | Modify bean fields |
| `bn close <id>` | Mark a bean complete |
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

## Design Decisions

**YAML over SQLite.** Human readability is the primary concern. You can open beans in your editor, browse them in a file tree, grep across them, and diff them in git.

**Sequential IDs with dot-notation.** `ls .beans/` shows structure. `3.2.yaml` is obviously a child of `3.yaml`. No UUIDs to memorize.

**Auto-indexed cache.** The index is rebuilt automatically when any YAML file changes. It's a performance optimization, not a source of truth. Eliminates an entire class of sync bugs.

**No fixed types.** Has children = container. No children = leaf. A "task" that gets decomposed becomes a "subgoal" implicitly. Types add ceremony without value.

**Stateless CLI.** Read files, write files, exit. No daemons, no lock files, no PID files. Index staleness is checked via mtime comparison.

**Git is sync.** No special merge drivers, no export formats, no sync branches. YAML diffs cleanly. Git handles versioning and collaboration natively.

## Tech Stack

- **Language:** Rust
- **CLI:** Clap 4 (derive macros)
- **Serialization:** Serde + serde_yaml
- **Error handling:** Anyhow
- **Timestamps:** Chrono

## Status

Planning complete. Implementation in progress.

## License

MIT
