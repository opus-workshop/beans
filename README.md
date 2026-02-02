# beans

A hierarchical task engine for autonomous AI agent coordination, built on file-backed storage.

No databases. No daemons. Just Markdown files with YAML frontmatter—queryable with standard Unix tools and version-controlled with git.

> **v0.1.0** — Production-ready task orchestration engine. See [Releases](https://github.com/opus-workshop/beans/releases) for downloads.

## Overview

Beans is a task management system designed to coordinate work between multiple AI agents. It provides:

- **Verify gates**: Tasks cannot close without proof of completion. The `verify` command must exit 0, or the task rolls back and a fresh agent retries.
- **Atomic claiming**: Race-condition-free task assignment ensures no two agents work the same task.
- **File-first design**: All data lives in `.beans/` as Markdown files with YAML frontmatter. No daemon, no database—just git-friendly files.
- **Hierarchical dependencies**: Tasks form DAGs with parent-child relationships and dependency tracking. Ready/blocked status derived automatically.
- **Lifecycle hooks**: Pre/post hooks for create, update, and close operations. CI gatekeeping via pre-close hooks.
- **Auto-archive**: Closed tasks move to `.beans/archive/YYYY/MM/` keeping the active directory clean.
- **Stateless CLI**: Lookup by mtime. Index is a cache, never the source of truth.

## Installation

```bash
# Build from source (installs bn and bctx binaries)
cargo install --git https://github.com/opus-workshop/beans

# Or clone and build
git clone https://github.com/opus-workshop/beans
cd beans
cargo build --release
./target/release/bn init my-project

# Optional: Install bpick fuzzy selector (requires fzf, jq)
cp tools/bpick ~/.local/bin/
```

## Feature Comparison

### Agent-Native Task Trackers

| Aspect | beans (this project) | [beads](https://github.com/steveyegge/beads) | [hmans/beans](https://github.com/hmans/beans) |
|--------|----------------------|----------------------------------------------|-----------------------------------------------|
| **Philosophy** | Simplicity, verify gates | Scale, multi-agent swarms | Agent-friendly, GraphQL |
| **Storage** | Markdown + YAML frontmatter | JSONL + SQLite cache | Markdown files |
| **ID scheme** | Hierarchical (`3.1` = child of `3`) | Hash-based (`bd-a1b2`) | UUID-based |
| **Verify gates** | ✓ Enforced (must exit 0) | ✗ Not enforced | ✗ Not enforced |
| **Daemon required** | ✗ Stateless CLI | ✓ Background sync | ✗ Stateless |
| **Direct file access** | ✓ `cat .beans/1-*.md` | ✗ Query via CLI | ✓ Markdown files |
| **Git diffs** | ✓ Clean, human-readable | ✗ JSONL harder to review | ✓ Clean |
| **Query interface** | CLI + JSON | CLI + JSON | GraphQL |
| **Built-in TUI** | ✗ (use `bpick`) | ✗ | ✓ |
| **Memory compaction** | ✗ Archive only | ✓ Semantic decay | ✗ |
| **Lifecycle hooks** | ✓ Pre/post create/update/close | ✗ | ✗ |
| **Auto-archive** | ✓ On close | ✗ | ✓ Archive command |
| **Task scalability** | Hundreds | Thousands+ | Hundreds |
| **Language** | Rust | Go | Go |
| **Best for** | Strict verification | Large swarms | GraphQL integrations |

### vs Traditional Issue Trackers

| Aspect | beans | Jira | GitHub Issues | Linear |
|--------|-------|------|---------------|--------|
| **Designed for** | AI agents | Humans | Humans | Humans |
| **Storage** | Local files | Cloud DB | Cloud DB | Cloud DB |
| **Offline support** | ✓ Full | ✗ Limited | ✗ Limited | ✗ Limited |
| **Git integration** | ✓ Native (files in repo) | ✗ External | ✓ Same platform | ✗ External |
| **Verify gates** | ✓ Enforced | ✗ Manual | ✗ Manual | ✗ Manual |
| **API for agents** | ✓ CLI + JSON | ✓ REST API | ✓ GraphQL | ✓ GraphQL |
| **Setup required** | `bn init` | Account + project | Repository | Workspace |
| **Cost** | Free | $$$$ | Free (public) | $$ |
| **Human UI** | Terminal / editor | Rich web UI | Web UI | Fast web UI |
| **Dependency graphs** | ✓ Built-in DAG | ✓ Via plugins | ✗ Limited | ✓ Built-in |
| **Auto-archive** | ✓ On close | ✗ Manual | ✗ Manual | ✗ Manual |
| **Hierarchy** | ✓ Parent-child IDs | ✓ Epics/stories | ✗ Flat | ✓ Projects/issues |

### When to Use What

| Use Case | Recommended |
|----------|-------------|
| Single agent, strict verification | **beans** (this project) |
| Multi-agent swarms at scale | **beads** |
| GraphQL-based agent integrations | **hmans/beans** |
| Human team with rich workflows | **Jira** or **Linear** |
| Open source project | **GitHub Issues** |
| Mixed human + agent workflow | **beans** or **beads** + sync to Jira/Linear |

## Design Rationale

Beans enforces a critical constraint: **proof-of-work**. Every task has a `verify` field—a shell command that must exit 0 for the task to close. This prevents:

- **Incomplete work**: Verification fails → task stays open
- **Stuck agents**: Verification fails → changes undo → fresh agent retries with full context
- **Silent failures**: Non-zero exit = explicit task failure, not "good enough"

For multi-agent systems, this architecture provides:

1. **Safety**: Atomic claiming prevents concurrent work on the same task
2. **Observability**: Attempt tracking shows retry history; index shows ready/blocked state
3. **Auditability**: Git log shows all task state changes; no hidden database state
4. **Simplicity**: Unix philosophy—everything is a file. Parse with standard tools. Compose with shell scripts.

## Architecture

### File Format

Beans use `{id}-{slug}.md` naming convention with YAML frontmatter:

```
.beans/
  config.yaml              # Project metadata
  index.yaml               # Auto-built index (cache, never edit)
  .hooks-trusted           # Hook trust marker (created by bn trust)
  hooks/                   # Lifecycle hook scripts
    pre-create
    post-create
    pre-close
  1-build-auth.md          # Task 1: "Build authentication"
  3-refactor.md            # Task 3: "Refactor parser"
  3.1-add-tests.md         # Task 3.1: "Add unit tests" (child of 3)
  archive/                 # Auto-archived closed tasks
    2026/01/
      2-old-task.md
```

### Bean Structure

```yaml
---
id: 3.1
title: Add comprehensive unit tests
status: open
priority: 2
parent: 3
dependencies:
  - 2
created_at: 2026-01-26T15:00:00Z
updated_at: 2026-01-26T15:00:00Z
attempts: 0
max_attempts: 3
description: |
  Write unit tests for the parser module.
  
  **Files to test:**
  - src/parser/lexer.rs
  - src/parser/ast.rs
  
  **Coverage target:** 80%+ lines covered
  
acceptance: |
  - Unit tests compile and pass
  - Coverage report shows ≥80% line coverage
  - All edge cases from issue #42 covered
  
verify: cargo test --lib parser && cargo tarpaulin --out Stdout --minimum 80
---

# Implementation Notes

Parser refactoring requires careful attention to backward compatibility.
See issue #42 for detailed specification.
```

**Fields:**
- `id`: Sequential integer with dot-notation for hierarchy
- `title`: Single-line summary
- `status`: `open` | `in_progress` | `closed`
- `priority`: 0-4 (0 = highest)
- `parent`: Parent task ID for hierarchy
- `dependencies`: List of task IDs that must close before this starts
- `attempts`: Number of close attempts
- `max_attempts`: Maximum attempts before manual escalation
- `description`: Agent prompt with context, file paths, acceptance criteria
- `acceptance`: Testable completion criteria
- `verify`: Shell command (must exit 0 to close)

The Markdown body (after frontmatter) is optional—for additional context or handoff notes.

### Index

The `.beans/index.yaml` file is a flattened cache built from all bean files:

```yaml
beans:
  - id: "1"
    title: "Build authentication"
    status: open
    priority: 2
    parent: null
    dependencies: []
    
  - id: "3.1"
    title: "Add unit tests"
    status: open
    priority: 2
    parent: "3"
    dependencies: ["2"]
```

Automatically rebuilt when any bean file's mtime exceeds the index mtime. Never edit manually.

## Commands

### Task Management

```bash
bn init [name]                          # Initialize .beans/ directory
bn create --title="..." [--parent ID]   # Create new task
bn show <id>                            # Display task details
bn edit <id>                            # Open task in $EDITOR
bn update <id> --title="..."            # Modify task fields
bn delete <id>                          # Remove task
```

### Agent Coordination

```bash
bn quick "title" --verify "cmd"         # Create and claim in one step (alias: bn q)
bn claim <id>                           # Atomically claim task (status: open → in_progress)
bn claim <id> --release                 # Release claimed task (status: in_progress → open)
bn close <id> [--reason "..."]          # Run verify, close if exits 0; archive on success
bn verify <id>                          # Test verify without closing
bn reopen <id>                          # Reopen closed task
bn unarchive <id>                       # Restore archived task to active beans
```

### Smart Selectors

Use `@`-prefixed selectors instead of explicit IDs:

```bash
bn show @latest                         # Most recently updated task
bn close @blocked                       # Close all blocked tasks
bn show @parent                         # Parent of current task (context-aware)
bn list @me                             # Tasks assigned to current user (BN_USER)
```

### Querying

```bash
bn status                               # Work overview: claimed, ready, blocked beans
bn ready                                # Show unblocked tasks sorted by priority
bn blocked                              # Show tasks blocked by unresolved dependencies
bn list [--status open] [--parent ID]   # List tasks with filters
bn tree [id]                            # Show hierarchy tree with status
bn graph [--format mermaid|dot]         # Dependency graph visualization
bn stats                                # Task counts, priority breakdown, progress
bn doctor                               # Health check—detect cycles, orphans
```

### Dependencies

```bash
bn dep add <id> <depends-on>            # Add dependency edge
bn dep remove <id> <depends-on>         # Remove dependency
bn dep list <id>                        # Show task's dependencies and dependents
bn dep tree [id]                        # Full dependency tree
bn dep cycles                           # Detect and report cycles
```

### Hooks

Beans supports lifecycle hooks—shell scripts that run before/after task operations:

```bash
bn trust                                # Enable hook execution (creates .beans/.hooks-trusted)
bn trust --check                        # Check if hooks are enabled
bn trust --revoke                       # Disable hooks
```

**Hook directory structure:**
```
.beans/hooks/
  pre-create      # Validate before task creation (exit non-zero to reject)
  post-create     # Notify after task creation
  pre-update      # Validate before task update
  post-update     # Notify after task update
  pre-close       # CI gatekeeper—runs before verify (can block close)
```

Hooks receive JSON via stdin containing the full bean context. Pre-hooks can block operations by exiting non-zero.

### Context Assembly

```bash
bn context <id>                         # Assemble context from task description (extracts file paths)
bctx <id>                               # Standalone version of bn context
bpick                                   # Interactive fuzzy selector (requires fzf, jq)
```

**Usage examples:**
```bash
bn context 14 | llm "Implement this"    # Pipe task context to LLM
bn close $(bpick)                       # Interactively select and close a task
```

### Maintenance

```bash
bn sync                                 # Force index rebuild from files
```

## Design Decisions

**File-based over database:**
Human readability and git compatibility. Sacrifices query performance for operational simplicity.

**Sequential IDs with dot-notation:**
Hierarchy visible in filenames. `3.2` is obviously a child of `3`. No metadata lookup required.

**Verify gates (mandatory):**
Forces explicit proof of work. Prevents incomplete tasks from closing. Enables safe agent retries.

**Atomic claiming:**
File system rename operation ensures only one agent can claim a task. No locks, no contention.

**Stateless operations:**
No daemon, no connection pooling, no background sync. Each command reads files, modifies them, exits. Index staleness checked via mtime.

## Use Cases

- **Epic coordination**: Break features into tasks. Assign to agent swarms. Track readiness without manual polling.
- **Hierarchical workflows**: Top-level goals → subgoals → agent-executable leaves. Parent beans document "why"; leaves document "what".
- **Verification-driven development**: Every task must prove completion. Tests, builds, lint checks—all enforced.
- **Multi-agent orchestration**: Atomic claiming prevents conflicts. Verify gates prevent incomplete work. Attempt tracking prevents infinite retries.
- **Audit trails**: Full git history of task state. Who claimed it? When did it close? Why did it fail?

## Testing

```bash
cargo test              # Run all tests (420+ passing)
cargo test --lib        # Unit tests only
```

## License

Apache 2.0. See [LICENSE](LICENSE).

## Contributing

Contributions welcome. Please open an issue for feature requests or bugs.
