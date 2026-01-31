---
project: beans
binary: bn
version: 0.1.0
---

# Beans Schema Reference

## Project Layout

```
.beans/
  config.yaml          # project settings
  bean.yaml            # root goal — the strategic "what and why"
  index.yaml           # auto-rebuilt cache (never edit manually)
  1.yaml               # task file
  3.2.yaml             # child task (parent 3)
```

## Bean Data Model

### id
- **Type:** integer (dot-notation for children)
- **Format:** Sequential, e.g., `1`, `3`, `3.2`, `3.2.1`
- **Description:** Auto-incremented from `config.yaml:next_id`. Children append `.N` to parent's ID.

### title
- **Type:** string (required)
- **Description:** Short human-readable summary

### status
- **Type:** enum - `open` | `in_progress` | `closed`
- **Default:** `open`
- **Description:** Three states only. "blocked" is derived from dependencies, never stored.

### priority
- **Type:** integer (0-4)
- **Default:** 2
- **Description:** P0 (highest) to P4 (lowest)

### description
- **Type:** string
- **Description:** Rich context. For leaf beans, this is the agent prompt with file paths, snippets, and patterns.

### acceptance
- **Type:** string
- **Description:** What "done" looks like. Testable criteria agents can verify.

### verify
- **Type:** string (shell command)
- **Description:** Command that must exit 0 for `bn close` to succeed. No force flag. Proof of work.
- **Examples:**
  - `cargo test --lib commands::list`
  - `npm test -- --grep 'auth'`

### notes
- **Type:** string
- **Description:** Handoff notes written by completing agent. What was created, what downstream beans need to know.

### design
- **Type:** string
- **Description:** Design decisions and rationale.

### dependencies
- **Type:** array of bean IDs
- **Description:** Beans that must be closed before this one can start (DAG edges).

### parent
- **Type:** string (bean ID)
- **Description:** Parent bean ID. Defines hierarchy.

### labels
- **Type:** array of strings
- **Description:** Freeform tags for filtering

### assignee
- **Type:** string
- **Description:** Who is working on this (agent ID or name)

### attempts
- **Type:** integer
- **Default:** 0
- **Description:** Number of close attempts. Incremented on each attempt.

### max_attempts
- **Type:** integer
- **Description:** Maximum close attempts. Prevents infinite retry loops.

### created_at / updated_at / closed_at
- **Type:** datetime (ISO 8601)
- **Description:** Timestamps (auto-managed)

## Commands Reference

| Command | Purpose |
|---------|---------|
| `bn create [title]` | Create a new bean |
| `bn show <id>` | Display bean details |
| `bn list [flags]` | List beans with filtering |
| `bn update <id> [flags]` | Update bean fields |
| `bn close <id>` | Close bean (runs verify) |
| `bn verify <id>` | Run verify command without closing |
| `bn claim <id>` | Atomically claim bean for work |
| `bn ready` | Show ready beans (open, no blockers) |
| `bn blocked` | Show blocked beans |
| `bn tree [id]` | Hierarchical tree view |
| `bn dep add/remove/list/tree` | Dependency management |
| `bn graph [--format]` | Dependency graph (mermaid/dot) |
| `bn stats` | Project statistics |
| `bn doctor` | Health check |
| `bn sync` | Force index rebuild |
| `bn init [name]` | Initialize beans project |

## Design Principles

- **Everything is a file** — YAML files you can read, edit, grep, git-diff
- **No daemon, no database** — Stateless CLI, mtime-based staleness
- **Git is sync** — `git add .beans/ && git commit` is the integration
- **Verify gates** — Tasks prove they're done, not just marked closed
- **Atomic claiming** — Race-condition-free agent coordination
