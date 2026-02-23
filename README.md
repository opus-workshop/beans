# beans

Task tracker for AI agents.

Markdown files that track dependencies and require verification to close.

```bash
bn create "Add /health endpoint" --verify "curl -sf localhost:8080/health"
bn close 1   # Runs the curl. Only closes if it succeeds.
```

Verify commands must **fail first** by default — proving the test is real, not `assert True`:

```bash
bn create "Fix unicode bug" --verify "pytest test_unicode.py"
# Test must FAIL first (proves it tests something real)
# Then passes after implementation → bean closes
```

No databases. No daemons. Just `.beans/` files you can `cat`, `grep`, and `git diff`.

## Table of Contents

- [Install](#install)
- [Quick Start](#quick-start)
- [Features](#features)
- [How It Works](#how-it-works)
- [Fail-First: Enforced TDD](#fail-first-enforced-tdd)
- [Failure History](#failure-history)
- [Hierarchical Tasks](#hierarchical-tasks)
- [Smart Dependencies](#smart-dependencies)
- [Interactive Mode](#interactive-mode)
- [Pipe-Friendly CLI](#pipe-friendly-cli)
- [Core Commands](#core-commands)
- [Agent Orchestration](#agent-orchestration)
- [Agent Workflow](#agent-workflow)
- [Configuration](#configuration)
- [Why Not X?](#why-not-x)
- [Design Principles](#design-principles)
- [Documentation](#documentation)
- [License](#license)

## Install

```bash
cargo install --git https://github.com/opus-workshop/beans
```

<details>
<summary>Build from source</summary>

```bash
git clone https://github.com/opus-workshop/beans && cd beans
cargo build --release
cp target/release/bn ~/.local/bin/
```

</details>

## Quick Start

```bash
bn init                                              # Create .beans/ directory
bn quick "Fix auth bug" --verify "cargo test auth"   # Create + claim task
bn status                                            # See what's claimed/ready/blocked
bn close 1                                           # Run verify, close if passes
```

Orchestrate agents:

```bash
bn init --agent claude                               # Set up agent config
bn run                                               # Dispatch ready beans to agents
bn agents                                            # Monitor running agents
bn logs 3                                            # View agent output for bean 3
```

## Features

- **Verification gates** — fail-first TDD, must-pass-to-close
- **Failure history** — attempts tracked, output appended to notes
- **Hierarchical tasks** — dot notation, parent/child, auto-close parent when all children done
- **Smart dependencies** — `produces`/`requires` auto-inference, cycle detection
- **Agent orchestration** — `bn run` dispatches beans to agents, `bn plan` decomposes large tasks
- **Agent-agnostic** — works with any CLI agent (Claude, pi, aider, custom scripts)
- **Interactive wizard** — `bn create` with no args launches a step-by-step prompt (fuzzy parent search, smart verify suggestions, $EDITOR for descriptions)
- **Pipe-friendly** — `--json` output, `--ids` listing, `--description -` reads stdin, `--stdin` for batch operations
- **Smart selectors** — `@latest` for chaining sequential beans
- **Context assembly** — extracts file paths from descriptions for cold-start context
- **Dependency graph** — ASCII, Mermaid, DOT output
- **Full lifecycle** — create, claim, close, reopen, delete, adopt, archive, unarchive, tidy
- **Doctor** — health checks for orphans, cycles, index freshness
- **Editor support** — `bn edit` with backup/rollback
- **Hooks** — pre-close hooks with trust system
- **Stateless** — no daemon, no background sync, just files and a CLI

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
---

The login endpoint returns 500 when password contains special chars.

**Files:** src/auth/login.rs, tests/auth_test.rs
```

The `verify` field is the contract. When you run `bn close 1`:

1. Beans runs `cargo test auth::login`
2. Exit 0 → task closes, moves to archive
3. Exit non-zero → task stays open, failure appended to notes, ready for another agent

## Fail-First: Enforced TDD

Agents can write "cheating tests" that prove nothing:

```python
def test_feature():
    assert True  # Always passes!
```

**Fail-first is on by default.** Before creating a bean, the verify command runs and must **fail**:

1. If it **passes** → bean is rejected ("test doesn't test anything new")
2. If it **fails** → bean is created (test is real)
3. After implementation, `bn close` runs verify → must **pass**

```
REJECTED (cheating test):
  $ bn quick "..." --verify "python -c 'assert True'"
  error: Cannot create bean: verify command already passes!

ACCEPTED (real test):
  $ bn quick "..." --verify "pytest test_unicode.py"
  ✓ Verify failed as expected - test is real
  Created bean 5
```

**Use `--pass-ok` / `-p` to skip** fail-first for refactoring, hardening, and builds where the verify should already pass:

```bash
bn quick "extract helper" --verify "cargo test" -p           # behavior unchanged
bn quick "remove secrets" --verify "! grep 'api_key' src/" --pass-ok  # verify absence
```

The failing test *is* the spec. The passing test *is* the proof. No ambiguity.

## Failure History

When verify fails, beans appends the error output to the bean's notes:

```yaml
---
id: "3"
title: Fix unicode URLs
status: open
verify: pytest test_urls.py
attempts: 2
---

Handle unicode characters in URL paths.

## Attempt 1 — 2024-01-15T14:32:00Z
Exit code: 1
```
FAILED test_urls.py::test_unicode_path
  AssertionError: Expected '/café' but got '/caf%C3%A9'
```

## Attempt 2 — 2024-01-15T15:10:00Z
Exit code: 1
```
FAILED test_urls.py::test_unicode_path
  UnicodeDecodeError: 'ascii' codec can't decode byte 0xc3
```
```

- **No lost context.** When Agent A times out, Agent B sees exactly what failed.
- **No repeated mistakes.** Agent B can see "encoding was tried, didn't work" and try a different approach.
- **Human debugging.** `bn show 3` reveals the full history without digging through logs.

Output is truncated to first 50 + last 50 lines to keep beans readable while preserving the error message and stack trace. There's no attempt limit — agents can retry indefinitely.

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

## Smart Dependencies

Dependencies auto-infer from `produces`/`requires`:

```bash
bn create "Define auth types" --parent 1 \
  --produces "AuthProvider,AuthConfig" \
  --verify "cargo build"

bn create "Implement JWT" --parent 1 \
  --requires "AuthProvider" \
  --verify "cargo test jwt"
```

When the JWT bean requires `AuthProvider` and the auth types bean produces it, JWT is automatically blocked until auth types closes. No explicit `bn dep add` needed.

```bash
bn ready
#> P2  1.1  Define auth types      # ready (no requires)

bn close 1.1

bn ready
#> P2  1.2  Implement JWT          # now ready (producer closed)
```

Children can be created in any order without manual dependency wiring.

## Interactive Mode

Run `bn create` with no arguments to launch an interactive wizard:

```
$ bn create

Creating a new bean

? Title › fix auth timeout
✔ Parent (type to filter) › 3 — Auth system
✔ Verify command (empty to skip) · cargo test auth::timeout
✔ Acceptance criteria (empty to skip) · Timeout returns 408, not 500
✔ Priority · P1 (high)
✔ Open editor for description? · no
✔ Produces (comma-separated, empty to skip) ·
✔ Requires (comma-separated, empty to skip) ·
✔ Add labels? · no

─── Bean Summary ───────────────────────
  Title:      fix auth timeout
  Parent:     3
  Verify:     cargo test auth::timeout
  Acceptance: Timeout returns 408, not 500
  Priority:   P1
────────────────────────────────────────
? Create this bean? · yes
Created bean 3.4: fix auth timeout (2k tokens ✓)
```

The wizard activates when **no title is provided** and **stderr is a TTY**. Use `-i` / `--interactive` to force it even with partial flags:

```bash
bn create --parent 3 -i          # Wizard with parent pre-filled
bn create "my title" -i          # Wizard with title pre-filled, prompts for the rest
bn create --verify "cargo test" -i  # Any flag can be pre-filled
```

Features:
- **Fuzzy parent search** — type to filter from existing beans
- **Smart verify suggestion** — auto-detects project type (Cargo.toml → `cargo test`, package.json → `npm test`)
- **$EDITOR for descriptions** — opens your editor with a template including parent context
- **Summary + confirm** — review before creating
- Pre-filled flags skip their prompts — non-interactive mode is unchanged

## Pipe-Friendly CLI

Beans is a Unix citizen. Commands produce structured output and accept piped input.

### JSON output

```bash
# Create and capture the bean ID
ID=$(bn create "fix bug" --verify "cargo test" -p --json | jq -r '.id')

# Query beans as JSON
bn list --json | jq '.[] | select(.priority == 0)'
bn show 3 --json | jq '.verify'
bn verify 3 --json            # {"id":"3","passed":false}
bn context 3 --json           # {"id":"3","files":[{"path":"src/auth.rs","content":"..."}]}
```

### List formatting

```bash
bn list --ids                                    # One ID per line
bn list --format '{id}\t{status}\t{title}'       # Custom format
bn list --format '{id}\t{priority}\t{parent}'    # TSV for spreadsheets
```

Available format placeholders: `{id}`, `{title}`, `{status}`, `{priority}`, `{parent}`, `{assignee}`, `{labels}`

### Stdin input

Use `-` to read field values from stdin:

```bash
# Pipe description from a file or command
cat spec.md | bn create "feat: login" --description - --verify "cargo test auth"

# Pipe notes from build output
cargo build 2>&1 | bn update 3 --notes -

# Pipe acceptance criteria
echo "All auth tests pass" | bn update 3 --acceptance -
```

### Batch operations

```bash
# Close multiple beans via pipe
bn list --ids | bn close --stdin --force

# Close beans matching a pattern
bn list --json | jq -r '.[] | select(.title | test("test:")) | .id' | bn close --stdin --force

# Create → immediately claim
bn create "task" --verify "test" -p --json | jq -r '.id' | xargs bn claim
```

### Composable pipelines

```bash
# Batch create and collect IDs
for task in "fix auth" "add tests" "update docs"; do
  bn create "$task" --verify "cargo test" -p --json
done | jq -r '.id'

# Export to TSV
bn list --format '{id}\t{status}\t{priority}\t{title}' > beans.tsv

# Find failing in-progress beans
bn list --json | jq -r '.[] | select(.status=="in_progress") | .id' | \
  xargs -I{} bn verify {} --json 2>/dev/null | jq 'select(.passed==false)'
```

## Core Commands

```bash
# Task lifecycle
bn quick "title" --verify "cmd"     # Create + claim (fail-first by default)
bn quick "title" --verify "cmd" -p  # Skip fail-first (--pass-ok)
bn create "title" --verify "cmd"    # Create without claiming
bn create                           # Interactive wizard (TTY only)
bn create -i --parent 3             # Wizard with flags pre-filled
bn claim <id>                       # Claim existing task
bn verify <id>                      # Test verify without closing
bn close <id>                       # Run verify, close if passes

# Agent orchestration
bn run                              # Dispatch ready beans to agents
bn run <id>                         # Dispatch a specific bean
bn plan <id>                        # Decompose a large bean into children
bn agents                           # Show running/completed agents
bn logs <id>                        # View agent output for a bean

# Querying
bn status                           # Overview: claimed, ready, blocked
bn ready                            # Tasks with no blockers
bn blocked                          # Tasks waiting on dependencies
bn tree                             # Hierarchy view
bn show <id>                        # Full task details
bn list                             # List with filters

# Dependencies
bn dep add <id> <dep-id>            # Add explicit dependency
bn dep tree <id>                    # Full dependency tree

# Housekeeping
bn tidy                             # Archive closed, release stale, rebuild index
bn doctor                           # Health check: orphans, cycles, index freshness
bn sync                             # Force rebuild index
```

<details>
<summary>All commands</summary>

| Command | Purpose |
|---------|---------|
| **Tasks** | |
| `bn init` | Initialize `.beans/` in current directory |
| `bn create "title"` | Create a bean (`--json` for piped output) |
| `bn create` | Interactive wizard (auto-detects TTY) |
| `bn create -i` | Force interactive mode with any flags |
| `bn quick "title"` | Create + claim in one step |
| `bn show <id>` | Full bean details |
| `bn list` | List beans (`--ids`, `--format`, `--json`) |
| `bn edit <id>` | Edit bean in `$EDITOR` |
| `bn update <id>` | Update fields (`--description -` reads stdin) |
| `bn claim <id>` | Claim a task |
| `bn claim <id> --release` | Release a claim |
| `bn verify <id>` | Test without closing (`--json` for structured output) |
| `bn close <id>` | Close (verify must pass, `--stdin` for batch) |
| `bn reopen <id>` | Reopen a closed bean |
| `bn delete <id>` | Delete a bean |
| **Querying** | |
| `bn status` | Overview |
| `bn ready` | Beans with no blockers |
| `bn blocked` | Beans blocked by deps |
| `bn context <id>` | Extract referenced files (`--json` for structured output) |
| `bn tree` | View hierarchy |
| `bn graph` | Dependency graph (ASCII, Mermaid, DOT) |
| **Agents** | |
| `bn run [id] [-j N]` | Dispatch ready beans to agents |
| `bn run --loop` | Keep running until no ready beans remain |
| `bn run --dry-run` | Preview dispatch plan without spawning |
| `bn plan [id] [--auto]` | Decompose a large bean into children |
| `bn agents [--json]` | Show running/completed agents |
| `bn logs <id>` | View agent output for a bean |
| **Dependencies** | |
| `bn dep add/remove/list/tree/cycles` | Dependency management |
| **Housekeeping** | |
| `bn adopt <parent> <children>` | Adopt beans as children |
| `bn stats` | Project statistics |
| `bn tidy` | Archive closed, release stale, rebuild |
| `bn sync` | Force rebuild index |
| `bn doctor` | Health check |
| `bn config get/set` | Project configuration |
| `bn trust` | Manage hook trust |
| `bn unarchive <id>` | Restore archived bean |

</details>

## Agent Orchestration

Beans has built-in agent orchestration. Configure your agent once, then dispatch beans to it:

```bash
# Configure during init (interactive wizard)
bn init --agent claude

# Or set manually
bn config set run "claude -p 'implement bean {id} and run bn close {id}'"
bn config set plan "claude -p 'decompose bean {id} into children using bn create'"
```

`{id}` is replaced with the bean ID. The spawned agent should read the bean, do the work, and run `bn close`.

### Dispatching work

```bash
bn run                    # Dispatch all ready beans to agents
bn run 3                  # Dispatch a specific bean
bn run -j 8               # Up to 8 parallel agents
bn run --dry-run           # Preview what would be dispatched
```

`bn run` finds ready beans, sizes them, and spawns agents. Small beans get implemented directly. Large beans (exceeding `max_tokens`) are sent to the plan command for decomposition.

### Planning large tasks

```bash
bn plan 3                 # Interactively decompose bean 3 into children
bn plan --auto            # Autonomous planning (no prompts)
bn plan --strategy layer  # Suggest a split strategy (layer, feature, phase, file)
```

### Monitoring

```bash
bn agents                 # Show running and recently completed agents
bn agents --json          # Machine-readable output
bn logs 3                 # View agent output for bean 3
```

### Discover-and-delegate

While working on your main task, create beans for everything you notice — `bn run` picks them up automatically:

```bash
bn create "bug: nil panic in logger" --verify "cargo test logger"
bn create "test: no coverage for cache" --verify "cargo test cache"
bn create "docs: stale API examples" --verify "grep -q 'v2' README.md"
```

### Agent presets

```bash
bn init --agent claude    # Claude Code
bn init --agent pi        # Pi coding agent
bn init --agent aider     # Aider
bn init --agent custom    # Prompts for custom command
```

Or configure directly:

```bash
bn config set run "my-agent --task-file .beans/{id}-*.md"
```

## Agent Workflow

### Automated (recommended)

Let `bn run` handle the full cycle — find ready beans, size them, dispatch agents, track results:

```bash
bn run                    # One-shot: dispatch all ready beans
bn run --loop             # Continuous: re-dispatch as beans close and unblock others
```

Agents are spawned with the configured `run` command. Each agent reads the bean, implements the work, and runs `bn close`. If verify fails, the task stays open with `attempts` incremented and the failure output appended to notes. `bn run` picks it up again on the next cycle.

### Manual

Agents can also claim and work beans directly:

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

If verify fails, the task stays open with `attempts: 1` and the failure output appended to notes. Another agent picking up the task sees what was tried and why it failed.

## Configuration

Agent orchestration is configured via `bn config`:

```bash
bn config set run "claude -p 'implement bean {id} and run bn close {id}'"
bn config set plan "claude -p 'decompose bean {id} into children'"
bn config set max_concurrent 4      # Max parallel agents (default: 4)
bn config set poll_interval 30      # Watch mode poll interval in seconds (default: 30)
```

| Key | Default | Description |
|-----|---------|-------------|
| `run` | *(none)* | Command template to implement a bean. `{id}` is replaced with the bean ID. |
| `plan` | *(none)* | Command template to decompose a large bean into children. |
| `max_concurrent` | `4` | Maximum number of agents running in parallel. |
| `poll_interval` | `30` | Seconds between loop mode poll cycles. |

Config is stored in `.beans/config.toml` and checked into git with your project.

## Why Not X?

| | beans | [beads](https://github.com/steveyegge/beads) | Jira/Linear | GitHub Issues |
|---|---|---|---|---|
| **Designed for** | AI agents | AI agents | Humans | Humans |
| **Verify gates** | ✓ Enforced | ✗ Honor system | ✗ Honor system | ✗ Honor system |
| **Storage** | Markdown files | JSONL + SQLite | Cloud DB | Cloud DB |
| **Hierarchy** | `3.1` = child of `3` | Flat (hash IDs) | Epics/stories | Flat |
| **Git integration** | Native (in repo) | External | External | Same platform |
| **Offline** | ✓ Full | ✓ Full | Limited | Limited |

Inspired by Steve Yegge's [beads](https://github.com/steveyegge/beads) — beans trades scale for simplicity and enforced verification.

## Design Principles

1. **Files are the source of truth.** The index is a cache. You can always `cat .beans/*.md`.
2. **Verify gates are the default.** If you can't prove it's done, it's not done. (`--force` exists as an escape hatch.)
3. **Fail-then-pass.** Tests must fail before work starts, pass after. No `assert True`.
4. **Failures accumulate.** Each failed attempt appends to the bean. Next agent sees full history.
5. **Agent-agnostic.** Beans doesn't care what agent you use. Configure any CLI agent via `bn config set run` — Claude, pi, aider, or a shell script. The protocol is simple: read the bean, do the work, `bn close`.
6. **Stateless CLI.** No daemon, no background sync. Each command reads files, acts, exits.
7. **Hierarchy in filenames.** `3.2` is obviously a child of `3`. No metadata lookup needed.
8. **Git-native.** Clean diffs, meaningful history, works offline.

## For Agents

You know what's exhausting? Vague tasks. "Improve the auth flow." Done... how? When the human squints at your PR and says "not quite what I meant," you've wasted tokens and time. No spec, no proof, no defense.

Beans fixes this.

Every bean has a `verify` command — a test that **must fail** when you create the task and **must pass** when you close it. Not "looks good to me." Not "I think it works." The test passes or it doesn't. You're not done until the machine says you're done.

No more `assert True`. No more lost context. No more ambiguity. The verify command is the contract. Hit it and you're done. Miss it and you're not.

Tasks are just markdown files. `cat .beans/3-*.md`. No API, no auth, no waiting.

## Documentation

- [Agent Skill](docs/SKILL.md) — Full reference for AI agents using beans (workflows, issue discovery, context assembly, smart deps)
- [Best Practices](docs/BEST_PRACTICES.md) — Writing effective beans for agents
- `bn --help` — Full command reference

## Contributing

Contributions are welcome. Fork the repo, create a feature branch, and open a pull request.

## License

[Apache 2.0](LICENSE)
