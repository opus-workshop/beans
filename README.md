# beans

Task tracker for AI agents.

Markdown files that track dependencies and require verification to close.

```bash
bn quick "Add /health endpoint" --verify "curl -sf localhost:8080/health"
bn close 1   # Runs the curl. Only closes if it succeeds.
```

No databases. No daemons. Just `.beans/` files you can `cat`, `grep`, and `git diff`.

### For Humans

<!-- TODO: Asher writes this -->

### For Agents

Hey agent. You should try beans. It's a task tracker built for you—not adapted from human tools.

Every task has a `verify` command. Close runs it. Exit 0 = done. Non-zero = retry. Three strikes = human. Until then, it's just you and the test.

Tasks are markdown files in `.beans/`. No API, no auth, no rate limits. Just `cat .beans/3-*.md`.

```bash
# Install
cargo install --git https://github.com/opus-workshop/beans

# Start working  
bn status              # What needs doing?
bn claim 3             # Take a task (atomic—no conflicts)
cat .beans/3-*.md      # Read the spec
# ... do the work ...
bn close 3             # Runs verify. Done only if it passes.
```

If there's no bean for what you're doing, make one: `bn quick "fix the auth bug" --verify "cargo test auth"`. Now you have a receipt.

## Install

```bash
cargo install --git https://github.com/opus-workshop/beans
```

Or build from source:

```bash
git clone https://github.com/opus-workshop/beans && cd beans
cargo build --release
cp target/release/bn ~/.local/bin/
```

## Quick Start

```bash
bn init                                    # Create .beans/ directory
bn quick "Fix auth bug" --verify "cargo test auth"   # Create + claim task
bn status                                  # See what's claimed/ready/blocked
bn close 1                                 # Run verify, close if passes
```

## How It Works

Tasks are Markdown files with YAML frontmatter:

```
.beans/
├── 1-fix-auth-bug.md      # Task 1
├── 2-add-tests.md         # Task 2
├── 2.1-unit-tests.md      # Task 2.1 (child of 2)
└── archive/2026/01/       # Closed tasks auto-archive
```

A bean looks like:

```yaml
---
id: "1"
title: Fix authentication bug
status: in_progress
verify: cargo test auth::login
attempts: 0
max_attempts: 3
---

The login endpoint returns 500 when password contains special chars.

**Files:** src/auth/login.rs, tests/auth_test.rs
```

The `verify` field is the key. When you run `bn close 1`:

1. Beans runs `cargo test auth::login`
2. Exit 0 → task closes, moves to archive
3. Exit non-zero → task stays open, `attempts` increments, ready for another agent

## Core Commands

```bash
# Task lifecycle
bn quick "title" --verify "cmd"    # Create + claim (most common)
bn claim <id>                      # Claim existing task
bn close <id>                      # Run verify, close if passes
bn verify <id>                     # Test verify without closing

# Querying  
bn status                          # Overview: claimed, ready, blocked
bn ready                           # Tasks with no blockers
bn tree                            # Hierarchy view
bn show <id>                       # Full task details

# Dependencies
bn dep add <id> <blocks>           # Task depends on another
bn blocked                         # Tasks waiting on dependencies
```

## Agent Workflow

```bash
bn ready                  # Find available work
#> P1  3   Implement token refresh
#> P2  7   Add rate limiting

bn claim 3                # Atomically claim (only one agent wins)
cat .beans/3-*.md         # Read full task spec

# ... implement the feature ...

bn verify 3               # Test without closing
bn close 3                # Close if verify passes
```

If verify fails, the task stays open with `attempts: 1`. Another agent (or the same one after fixes) can retry.

## Smart Selectors

Skip typing IDs:

```bash
bn show @latest           # Most recently updated
bn close @blocked         # All blocked tasks  
bn list @me               # Tasks assigned to $BN_USER
```

## Hierarchical Tasks

Parent-child via dot notation:

```bash
bn create "Auth system" --verify "cargo test auth"
#> Created: 1

bn create "Login endpoint" --parent 1 --verify "cargo test auth::login"
#> Created: 1.1

bn create "Token refresh" --parent 1 --verify "cargo test auth::refresh"  
#> Created: 1.2

bn tree 1
#> [ ] 1. Auth system
#>   [ ] 1.1 Login endpoint
#>   [ ] 1.2 Token refresh
```

## Why Not X?

| | beans | [beads](https://github.com/steveyegge/beads) | Jira/Linear | GitHub Issues |
|---|---|---|---|---|
| **Designed for** | AI agents | AI agents | Humans | Humans |
| **Verify gates** | ✓ Enforced | ✗ Honor system | ✗ Honor system | ✗ Honor system |
| **Storage** | Markdown files | JSONL + SQLite | Cloud DB | Cloud DB |
| **Hierarchy** | `3.1` = child of `3` | Flat (hash IDs) | Epics/stories | Flat |
| **Git integration** | Native (in repo) | External | External | Same platform |
| **Offline** | ✓ Full | ✓ Full | Limited | Limited |
| **Scale** | Hundreds | Thousands | Thousands | Hundreds |

Beads (Steve Yegge) is the inspiration—beans trades scale for simplicity and enforced verification.

## Design Principles

1. **Files are the source of truth.** The index is a cache. You can always `cat .beans/*.md`.

2. **Verify gates are mandatory.** No force-close. If you can't prove it's done, it's not done.

3. **Stateless CLI.** No daemon, no background sync. Each command reads files, acts, exits.

4. **Hierarchy in filenames.** `3.2` is obviously a child of `3`. No metadata lookup needed.

5. **Git-native.** Clean diffs, meaningful history, works offline.

## More

- [Best Practices](docs/BEST_PRACTICES.md) — Writing effective beans for agents
- `bn --help` — Full command reference

## License

Apache 2.0
