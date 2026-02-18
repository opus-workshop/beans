# beans

Task tracker for AI agents.

Markdown files that track dependencies and require verification to close.

```bash
bn quick "Add /health endpoint" --verify "curl -sf localhost:8080/health"
bn close 1   # Runs the curl. Only closes if it succeeds.
```

Want to ensure agents write real tests, not `assert True`? Use `--fail-first`:

```bash
bn quick "Fix unicode bug" --verify "pytest test_unicode.py" --fail-first
# Test must FAIL first (proves it tests something real)
# Then passes after implementation → bean closes
```

No databases. No daemons. Just `.beans/` files you can `cat`, `grep`, and `git diff`.

### For Humans

Beans is a task tracker for AI agents, built by AI agents. Guided by Unix principles, compatibility with spec, goal, and test-driven development frameworks, and human auditability. Inspired by Steve Yegge's beads. 
 
### For Agents

You know what's exhausting? Vague tasks. "Improve the auth flow." Done... how? When the human squints at your PR and says "not quite what I meant," you've wasted tokens and time. No spec, no proof, no defense.

Beans fixes this.

Every bean has a `verify` command—a test that **must fail** when you claim the task and **must pass** when you close it. Not "looks good to me." Not "I think it works." The test passes or it doesn't. You're not done until the machine says you're done.

```bash
bn claim 3             # Verify runs. Must FAIL. (Proves work is needed.)
# ... you implement ...
bn close 3             # Verify runs. Must PASS. (Proves you did it.)
```

**No more `assert True`.** If someone writes a test that already passes, the bean is rejected. You can't game it. You can't cheat. The failing test *is* your spec, and the passing test *is* your proof.

**No more conflicts.** If you're running in parallel with other agents, you get your own worktree. Make your changes. When you close, it merges. Conflict? You resolve it—you know what you wrote.

**No more lost context.** Every failed attempt is recorded in the bean. If you pick up a task someone else abandoned, you see exactly what they tried and why it failed. No repeating the same mistakes.

**No more ambiguity.** The verify command is the contract. Hit it and you're done. Miss it and you're not.

```bash
bn quick "fix unicode URLs" --verify "pytest test_urls.py" --fail-first
# pytest fails → bean created
# you fix the bug
# pytest passes → bean closed
# receipt: test_urls.py proves you fixed it
```

Tasks are just markdown files. `cat .beans/3-*.md`. No API, no auth, no waiting.

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

## Features

- **Hierarchical tasks** — dot notation, parent/child, auto-close parent when all children done
- **Verification gates** — fail-first TDD, must-pass-to-close, force override
- **Failure history** — attempts tracked, output truncated & appended to notes
- **Smart dependencies** — produces/requires auto-inference, cycle detection
- **Smart selectors** — `@latest`, `@blocked`, `@me`, `@parent`
- **Context assembly** — extracts file paths from descriptions, assembles for agents
- **Hooks** — pre-close hooks with trust system
- **Git worktree support** — detection, commit, merge-to-main, cleanup
- **3-way bean merge** — field-level conflict resolution
- **Dependency graph** — ASCII, Mermaid, DOT output
- **Full lifecycle** — create, claim, close, reopen, delete, adopt, archive, unarchive, tidy
- **Doctor** — health checks for orphans, cycles, index freshness
- **Config** — per-project settings
- **Editor support** — `bn edit` with backup/rollback
- **Token estimation** — for context sizing

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

The `verify` field is the key. When you run `bn close 1`:

1. Beans runs `cargo test auth::login`
2. Exit 0 → task closes, moves to archive
3. Exit non-zero → task stays open, failure appended to notes, ready for another agent

When closing from a **worktree** (parallel agent), `bn close` also handles merging:

```
bn close 5 (from .spro/5/)
    │
    ├── 1. Run verify → must pass
    ├── 2. Commit changes in worktree
    ├── 3. Merge to main branch
    │       ├── Clean merge → continue
    │       └── Conflict → fail, agent resolves, retries
    ├── 4. Archive bean
    └── 5. Remove worktree
```

One command. Agent doesn't need to know about git worktrees or merging.

## Fail-First: Enforced TDD

Agents can write "cheating tests" that prove nothing:

```python
def test_feature():
    assert True  # Always passes!
```

### Option 1: Explicit flag (`--fail-first`)

```bash
bn quick "Fix bug" --verify "pytest test_bug.py" --fail-first
```

1. Before creating the bean, runs the verify command
2. If it **passes** → rejects the bean ("test doesn't test anything new")
3. If it **fails** → creates the bean (test is real)
4. After implementation, `bn close` runs verify → must **pass**

```
REJECTED (cheating test):
  $ bn quick "..." --verify "python -c 'assert True'" --fail-first
  error: Cannot create bean: verify command already passes!

ACCEPTED (real test):
  $ bn quick "..." --verify "pytest test_unicode.py" --fail-first
  ✓ Verify failed as expected - test is real
  Created bean 5
```

Works on both `bn quick` and `bn create`.

**Use `--fail-first` for new behavior** (features, bug fixes with regression tests):

```bash
bn quick "add unicode support" --verify "pytest test_unicode.py" --fail-first
bn quick "fix login crash" --verify "cargo test auth::special_chars" --fail-first
```

**Use regular `--verify` for everything else** (refactoring, hardening, builds):

```bash
bn quick "extract helper" --verify "cargo test"           # behavior unchanged
bn quick "remove secrets" --verify "! grep 'api_key' src/"  # verify absence
```

### Option 2: Automatic on claim (verify-on-claim)

When claiming any bean with a verify command:

```bash
bn claim 5
# → Runs verify automatically
# → Must FAIL to prove test is real
# → Records checkpoint for later merge
```

```
$ bn claim 5
Running verify (pre-flight): pytest test_feature.py
FAILED - test_feature.py::test_unicode
✓ Claim granted (test is real)
Checkpoint: abc123

$ bn claim 6  
Running verify (pre-flight): python -c 'assert True'
✗ Claim rejected: verify already passes
  Nothing to do, or test doesn't test anything
```

This enforces TDD automatically—no flag needed. Use `--force` to override.

## Parallel Agents with Isolation

When multiple agents work in parallel (via [spro](https://github.com/anthropics/spro)), each gets an isolated git worktree:

```
spro run 5  (parent with children 5.1, 5.2, 5.3)
    │
    ├─► .spro/5.1/  (agent A's worktree)
    ├─► .spro/5.2/  (agent B's worktree)  
    └─► .spro/5.3/  (agent C's worktree)
```

**Why isolation matters:**
- Agents don't step on each other's changes
- Each verify runs in a clean, known state
- Conflicts detected at merge time, not randomly mid-work

**The flow:**

```bash
# Spro creates worktree
git worktree add .spro/5.1 HEAD

# Agent works in isolation
cd .spro/5.1
bn claim 5.1        # Verify must FAIL (proves test is real)
# ... implement ...
bn close 5.1        # Verify must PASS, then auto-merge to main

# Clean up
git worktree remove .spro/5.1
```

**Merge strategy:**
1. Try `git merge` (auto-resolves non-overlapping changes)
2. If conflict: same agent resolves (it knows its own intent)
3. If still stuck: escalate to human

## Core Commands

```bash
# Task lifecycle
bn quick "title" --verify "cmd"    # Create + claim (most common)
bn quick "title" --verify "cmd" --fail-first  # TDD: test must fail first
bn claim <id>                      # Claim existing task
bn close <id>                      # Run verify, close if passes
bn verify <id>                     # Test verify without closing

# Querying  
bn status                          # Overview: claimed, ready, blocked
bn ready                           # Tasks with no blockers
bn tree                            # Hierarchy view
bn show <id>                       # Full task details

# Housekeeping
bn tidy                            # Archive closed beans, release stale in-progress, rebuild index
bn tidy --dry-run                  # Preview without changing files
bn sync                            # Force rebuild index only

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

If verify fails, the task stays open with `attempts: 1` and the failure output appended to notes. Another agent picking up the task sees what was tried and why it failed.

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

**Why this matters:**

- **No lost context.** When Agent A times out, Agent B sees exactly what failed.
- **No repeated mistakes.** Agent B can see "encoding was tried, didn't work" and try a different approach.
- **Human debugging.** `bn show 3` reveals the full history without digging through logs.

Output is truncated to first 50 + last 50 lines (with a "lines omitted" note) to keep beans readable while preserving the error message and stack trace.

There's no attempt limit—agents can retry indefinitely. The failure history is the feedback loop.

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

When JWT bean requires `AuthProvider` and auth types bean produces it, JWT is automatically blocked until auth types closes. No explicit `bn dep add` needed.

```bash
bn ready
#> P2  1.1  Define auth types      # ready (no requires)

bn close 1.1

bn ready  
#> P2  1.2  Implement JWT          # now ready (producer closed)
```

This solves race conditions in parallel agent decomposition—children can be created in any order without manual dependency wiring.

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

3. **Fail-then-pass.** Tests must fail before work starts, pass after. No `assert True`.

4. **Failures accumulate.** Each failed attempt appends to the bean. Next agent sees full history.

5. **Isolation by default.** Parallel agents get worktrees. No stepping on each other.

6. **Stateless CLI.** No daemon, no background sync. Each command reads files, acts, exits.

7. **Hierarchy in filenames.** `3.2` is obviously a child of `3`. No metadata lookup needed.

8. **Git-native.** Clean diffs, meaningful history, worktree isolation, works offline.

## More

- [Best Practices](docs/BEST_PRACTICES.md) — Writing effective beans for agents
- `bn --help` — Full command reference
- BEANS = Bounded Executable Agent Node System                          

## License

Apache 2.0
