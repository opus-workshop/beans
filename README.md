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
- **Stateless CLI**: Lookup by mtime. Index is a cache, never the source of truth.

## Installation

```bash
# Build from source
cargo install --git https://github.com/opus-workshop/beans

# Or clone and build
git clone https://github.com/opus-workshop/beans
cd beans
cargo build --release
./target/release/bn init my-project
```

## Feature Comparison

| Aspect | beans | [hmans/beans](https://github.com/hmans/beans) |
|--------|-------|-------|
| **Storage backend** | YAML/Markdown files | SQLite + JSONL + daemon |
| **Verify gates** | ✓ Enforced | ✗ Not enforced |
| **Atomic claiming** | ✓ Via file operations | ✗ No guarantee |
| **Direct file access** | ✓ `cat .beans/1-*.md` | ✗ CLI-only |
| **ID scheme** | Hierarchical (3.2 = child of 3) | Hash-based |
| **Git compatibility** | ✓ Clean diffs, mergeable | ✗ Not git-friendly |
| **Daemon required** | ✗ Stateless | ✓ Background service |
| **Task scalability** | Hundreds (optimal) | Thousands+ (indexed) |
| **Scope** | Agent coordination | Full issue tracking |

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
  1-build-auth.md          # Task 1: "Build authentication"
  3-refactor.md            # Task 3: "Refactor parser"
  3.1-add-tests.md         # Task 3.1: "Add unit tests" (child of 3)
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
bn show <id>                            # Display task (YAML)
bn update <id> --title="..."            # Modify task fields
bn delete <id>                          # Remove task
```

### Agent Coordination

```bash
bn claim <id>                           # Atomically claim task (status: open → in_progress)
bn release <id>                         # Release claimed task (status: in_progress → open)
bn close <id> [--reason "..."]          # Run verify, close if exits 0; undo if fails
bn verify <id>                          # Test verify without closing
bn reopen <id>                          # Reopen closed task
```

### Querying

```bash
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
cargo test              # Run all tests (230+ passing)
cargo test --lib       # Unit tests only
```

## License

Apache 2.0. See [LICENSE](LICENSE).

## Contributing

Contributions welcome. Please open an issue for feature requests or bugs.
