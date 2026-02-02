---
name: beans
description: >
  Your task system. At session start, check if .beans/ exists and run `bn status` to see claimed/ready/blocked work.
  When a user asks for non-trivial work, create a bean first with `bn quick "task" --verify "cmd"`.
  Beans are designed for you (AI agents)—they provide context, track attempts, and prevent incomplete work via verification gates.
allowed-tools: Bash(bn:*), Read, Edit, Write
---

# Beans Task Engine

Beans is a hierarchical task engine for AI agents. Tasks have verify gates—shell commands that must exit 0 to close.

## Detection

Check if beans is available:
```bash
ls .beans/ 2>/dev/null && echo "beans active"
```

## Session Start

Always begin with:
```bash
bn status
```

This shows claimed, ready, and blocked work. If work exists that matches your task, claim it. **If not, create one.**

## Default: Create a Bean First

When a user asks you to do something non-trivial, your reflex should be:

```bash
bn quick "What user asked for" --verify "test command"
```

**Create a bean when:**
- The task involves changing code
- It has success criteria you can verify
- It might take more than a few commands
- You want to track what you did

**Skip beans only for:**
- Pure questions/explanations
- Single-command lookups
- Trivial one-line fixes

The bean becomes your receipt—proof of what was requested, what you did, and that it worked.

## Fail-First: Enforced TDD (Prevents Cheating Tests)

Use `--fail-first` to ensure your test is real, not a no-op like `assert True`:

```bash
bn quick "fix unicode URLs" --verify "pytest test_unicode.py" --fail-first
bn create "fix unicode URLs" --verify "pytest test_unicode.py" --fail-first
```

Works on both `bn quick` and `bn create`.

**How it works:**
1. Verify command runs BEFORE bean creation → must FAIL
2. If verify passes already, bean is REJECTED (test doesn't test anything new)
3. Bean created only if test fails (proving it tests real behavior)
4. After implementation, `bn close` runs verify → must PASS

**Example - Cheating test rejected:**
```bash
$ bn quick "fix bug" --verify "python -c 'assert True'" --fail-first
Running verify (must fail): python -c 'assert True'
error: Cannot create bean: verify command already passes!
```

**Example - Real test accepted:**
```bash
$ bn quick "fix unicode" --verify "pytest test_unicode.py::test_fetch" --fail-first
Running verify (must fail): pytest test_unicode.py::test_fetch
FAILED test_unicode.py::test_fetch - URLError...
✓ Verify failed as expected - test is real
Created and claimed bean 5: fix unicode (by pi-agent)
```

**When to use `--fail-first`:**
- Writing new features with tests
- Bug fixes where you write a regression test first
- Any TDD workflow

**When to skip `--fail-first`:**
- Build commands (`cargo build`, `make`)
- Refactoring (tests should already pass)
- Tasks without test-based verification

## Core Workflow

### 1. Find Work
```bash
bn ready                         # Tasks with no blockers
bn status                        # Full overview
```

### 2. Claim a Task
```bash
bn claim <id>                    # Atomic claim
cat .beans/<id>-*.md             # Read full spec
```

The bean file contains:
- **description**: What to build and why
- **acceptance**: Concrete criteria for "done"
- **verify**: Command that must exit 0

### 3. Work on It
```bash
bn verify <id>                   # Test if done (without closing)
bn update <id> --note "..."      # Log progress
```

### 4. Close It
```bash
bn close <id>                    # Runs verify, closes if exit 0
git commit -m "[beans-<id>] ..."
```

### 5. If Stuck
```bash
bn claim <id> --release          # Release for retry
bn update <id> --note "why"      # Explain blocker
```

## Creating Beans

**Do this early, not as an afterthought.** When a task lands, create the bean before diving into implementation:

```bash
bn quick "Title" --verify "cmd"   # Most common: create, claim, start working
bn create "Title"                 # New top-level bean (will claim separately)
bn create "Title" --parent <id>   # Child bean under existing work
```

**Good beans have:**
- File paths in description (no exploration needed)
- Testable acceptance criteria
- Verify command that proves completion
- Right size: 1-5 files, completable in one session

## Discovering Issues

While working, you'll notice things that need attention but aren't your current task. Handle them correctly:

### Is It Blocking You?

```
Issue blocks my current task?
  YES → Fix it as part of your work (or add as dependency)
  NO  → Report it, keep going
```

### Reporting Issues

Create a bean immediately, then continue your original task:

```bash
bn create "<type>: <what needs doing>" --verify "<command that passes when done>"
```

**Do not fix unrelated issues.** Create the bean and move on. A watcher will spawn an agent to handle it.

### Issue Types

| Type | Example |
|------|---------|
| `bug:` | `bug: http client crashes on unicode URLs` |
| `test:` | `test: no coverage for auth edge cases` |
| `docs:` | `docs: API endpoints undocumented` |
| `refactor:` | `refactor: duplicate code in handlers` |
| `perf:` | `perf: N+1 query in user list` |
| `security:` | `security: API key exposed in logs` |
| `chore:` | `chore: upgrade deprecated dependency` |

### Good Issue Beans

A good issue bean has:

1. **Typed title**: `<type>: <component> <problem/need>`
2. **Verify command**: Passes when the issue is resolved
3. **Context**: Relevant files if obvious

```bash
# Bug - test that should pass after fix
bn create "bug: login fails when email has plus sign" \
  --verify "cargo test test_login_plus_email"

# Missing test - test file should exist and pass
bn create "test: no coverage for payment refunds" \
  --verify "cargo test payment::refund"

# Docs - doc file should exist or contain content
bn create "docs: README missing setup instructions" \
  --verify "grep -q '## Setup' README.md"

# Refactor - code quality check
bn create "refactor: handlers have duplicate error logic" \
  --verify "test \$(grep -r 'handle_error' src/handlers | wc -l) -le 1"

# Perf - benchmark or query count
bn create "perf: N+1 query on user list endpoint" \
  --verify "./scripts/count-queries.sh /users | grep -q '^1$'"

# Security - absence of bad pattern
bn create "security: API keys logged in debug output" \
  --verify "! grep -r 'api_key.*debug\|log.*api_key' src/"

# Chore - version check
bn create "chore: upgrade deprecated serde version" \
  --verify "cargo tree -p serde | grep -q '1.0.200'"
```

### Example Workflow

You're working on bean 14 (add caching feature). You notice several issues:

```bash
# Notice logging is broken
bn create "bug: logger crashes on unicode" --verify "cargo test logger::unicode"
# Bean 15 created (unclaimed - watcher will spawn agent)

# Notice no tests for the module you're reading
bn create "test: cache module has no tests" --verify "cargo test cache::"
# Bean 16 created (unclaimed)

# Notice outdated docs
bn create "docs: cache docs reference old API" --verify "grep -q 'cache.get_or_set' docs/cache.md"
# Bean 17 created (unclaimed)

# Continue your actual work
bn verify 14  # back to your task
```

Three issues captured. None forgotten. You stayed focused.

### Why This Matters

- **Focus**: You finish what you started
- **Tracking**: Nothing gets forgotten
- **Parallelism**: Watcher spawns agents for new beans automatically
- **Visibility**: `bn ready` shows all discovered work
- **History**: Record of what was found and when

The flywheel: issues spawn beans → watcher spawns agents → agents resolve issues → agents discover more issues.

## Bean Anatomy

```yaml
id: '3.1'
title: Implement feature X
status: open                     # open | in_progress | closed
priority: 1                      # 0-4 (0 = critical)
parent: '3'                      # Hierarchy
dependencies: ['2.1']            # Blocking relationships

description: |
  ## Context
  Why we're doing this...
  
  ## Task
  1. Add X to file Y
  2. Write tests in Z
  
  ## Files
  - src/foo.rs
  - tests/foo_test.rs

acceptance: |
  - Feature does X
  - Tests pass
  - No regressions

verify: cargo test foo
```

## Smart Dependencies

Beans automatically infer dependencies from `produces` and `requires` fields:

```yaml
# Bean 14.1 - no dependencies
produces:
  - AuthProvider
  - AuthConfig

# Bean 14.2 - automatically blocked by 14.1
requires:
  - AuthProvider
```

When decomposing work, specify what each bean produces and requires:

```bash
bn create "Define auth types" --parent 14 \
  --produces "AuthProvider,AuthConfig" \
  --verify "cargo build"

bn create "Implement JWT" --parent 14 \
  --requires "AuthProvider" \
  --produces "JwtProvider" \
  --verify "cargo test jwt"
```

**How it works:**
- `bn ready` checks `requires` vs sibling `produces`
- If bean A requires X and sibling B produces X → A blocked until B closed
- No explicit `bn dep add` needed
- Solves race conditions in parallel decomposition

**Explicit deps still work:** You can still use `bn dep add` for dependencies that don't fit the produces/requires model.

## Parent Beans & Parallel Work

When you have multiple related tasks that can be worked in parallel, create a parent bean:

```bash
# Create parent with verify that checks all children are closed
bn create "Feature X - All Components" \
  --description "Parent for parallel implementation" \
  --verify "test \$(bn list --parent <id> --status closed | wc -l) -ge N"

# Create children under the parent (auto-numbered as P.1, P.2, etc.)
bn create "Component A" --parent 14 --verify "test -f src/a.rs"  # becomes 14.1
bn create "Component B" --parent 14 --verify "test -f src/b.rs"  # becomes 14.2
```

**Naming convention**: Children are numbered `<parent>.<n>`:
- Parent 211 → Children 211.1, 211.2, 211.3...
- Files: `211.1-slug.md`, `211.2-slug.md`...

**To adopt existing beans under a parent** (planned: `bn adopt`):
```bash
# Currently manual - bean 212 tracks native support
# See bean 212 for bn adopt command proposal
```

**Then use spro for parallel execution:**
```bash
spro run <parent-id> --dry-run   # See spro plan
spro run <parent-id>             # Spawn parallel agents
spro run <parent-id> -j 8        # More parallelism
```

See the `spro` skill for full details on parallel agent orchestration.

## Key Commands

| Command | Purpose |
|---------|---------|
| `bn status` | Overview of work |
| `bn quick "..." --verify "..."` | **User asks for something → create bean first** |
| `bn quick/create ... --fail-first` | **TDD mode: test must fail first** |
| `bn ready` | Tasks with no blockers |
| `bn claim <id>` | Claim existing task |
| `bn verify <id>` | Test verify without closing |
| `bn close <id>` | Close (verify must pass) |
| `bn update <id> --note "..."` | Log progress |
| `bn tree` | View hierarchy |
| `bn tree <id>` | View subtree under parent |
| `bn dep add <id> <dep>` | Add dependency |
| `bn list --parent <id>` | List children of parent |

## Smart Selectors

```bash
bn show @latest                  # Most recent bean
bn list @ready                   # All ready beans
bn list @blocked                 # All blocked beans
```

## The Verify Gate

**This is the core principle.** You cannot close a bean without proof of completion. If verify fails:
- Task stays open
- Attempts counter increments
- Another agent can retry

This prevents incomplete work from slipping through.

## Context Assembly

Get all files referenced in a bean's description:
```bash
bn context <id>                  # Output file contents as markdown
bctx <id>                        # Standalone version
```

This solves cold-start: instead of exploring, you get all relevant files immediately.

## Handoff Protocol

When finishing or releasing work:
```bash
bn update <id> --note "Implemented X in Y, tested with Z"
```

Notes help the next agent (or your future self) understand what happened.

## Bean Watcher (Autonomous Flywheel)

The bean watcher enables autonomous agent coordination:

```bash
# Start the watcher (runs continuously)
~/.pi/agent/skills/beans/bean-watcher.sh

# Or run once for testing
~/.pi/agent/skills/beans/bean-watcher.sh --once --dry-run
```

**How it works:**
1. Polls `bn ready` every 30 seconds
2. For each unclaimed bean, counts context tokens
3. If < 30k tokens → `spro spawn` to implement
4. If ≥ 30k tokens → `spro spawn` to decompose

**The flywheel:**
```
Agent discovers issue → bn create "bug: ..."
                              ↓
                     Bean created (unclaimed)
                              ↓
                     Watcher sees it
                              ↓
           < 30k tokens → spawn agent to implement
           ≥ 30k tokens → spawn agent to decompose
                              ↓
                     Agent works, may discover more issues
                              ↓
                     (cycle continues)
```

**Why this works:**
- Agents report issues with `bn create` (not `bn quick`)
- Smart dependencies auto-block children until producers close
- No race conditions in decomposition
- No human coordination needed
