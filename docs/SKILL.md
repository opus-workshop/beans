---
name: beans
description: >
  A task tracker with verify gates. Default action: `bn create "task" --verify "cmd"`.
allowed-tools: Bash, Read, Edit, Write
---

# Beans Task Engine

Beans is a task tracker where every task has a verify gate — a shell command that must exit 0 to close.

The magic: a well-written bean is a complete agent prompt. The description has the context, the file paths tell you where to look, the acceptance criteria define done, and the verify command proves it. An agent can pick up any bean cold and execute it without searching the codebase.

```bash
bn show <id>       # The prompt: what to do, why, and how to verify
bn context <id>    # The code: all files referenced in the description
```

These two commands are all an agent needs to start working.

## Create a Bean First

When a user asks for non-trivial work, create a bean before you start:

```bash
bn create "What they asked for" --verify "test command"
```

Then claim it and work:

```bash
bn claim @latest
```

Or use `bn quick` to create and claim in one step:

```bash
bn quick "What they asked for" --verify "test command"
```

### When to Create a Bean

| Situation | Command |
|-----------|---------|
| Fix a bug | `bn create "bug: ..." --verify "<test>"` |
| Add a feature | `bn create "feat: ..." --verify "<test>"` |
| Refactor code | `bn create "refactor: ..." --verify "<test>" -p` |
| Add tests | `bn create "test: ..." --verify "<test>"` |
| Update docs | `bn create "docs: ..." --verify "grep -q '...' <file>" -p` |
| Multi-step task | Always |
| Found issue while working | `bn create "bug: ..." --verify "<test>"` (don't claim — stay focused) |

### When NOT to Create a Bean

- Pure questions or explanations
- Single-command lookups
- Trivial one-line fixes

**When in doubt, create one.** It takes 2 seconds. Untracked work costs everything.

## Writing Good Beans

A good bean is a good prompt. Include enough context that any agent can pick it up cold:

```bash
bn create "title" --verify "cmd" --description "## Context
<Why this needs doing — 1-2 sentences>

## Task
1. <Concrete step>
2. <Concrete step>

## Files
- <path/to/file.rs> (<what changes>)
- <path/to/test.rs> (<what to test>)

## Edge Cases
- <What should fail and how>"
```

**Include**: file paths, what to change in each, edge cases, patterns to follow ("see src/auth.rs").
**Avoid**: vague directives ("make it work"), assumed context (every bean is read cold).

The description is the prompt. The better you write it, the less the agent has to figure out.

## Choosing a Verify Command

Scan the project to pick the right pattern:

```bash
ls Cargo.toml package.json pyproject.toml go.mod Makefile 2>/dev/null
```

### By Project Type

| Detected | Pattern | Example |
|----------|---------|---------|
| `Cargo.toml` | `cargo test <module>::<test>` | `cargo test auth::test_login` |
| `package.json` + jest | `npx jest --testPathPattern "<pat>"` | `npx jest auth` |
| `package.json` + vitest | `npx vitest run <pat>` | `npx vitest run auth` |
| `pyproject.toml` | `pytest <file> -k "<pat>"` | `pytest tests/test_auth.py -k login` |
| `go.mod` | `go test ./... -run <Pat>` | `go test ./pkg/auth -run TestLogin` |
| `Makefile` | `make <target>` | `make test-auth` |

### By Task Type

| Task | Strategy |
|------|----------|
| Fix a bug | Test that reproduces the bug: `<test-cmd> <specific_test>` |
| Add a feature | Tests for the feature: `<test-cmd> <feature_module>` |
| Refactor | Broad existing tests with `-p`: `<test-cmd>` |
| Add docs | Check content exists with `-p`: `grep -q '<content>' <file>` |
| Remove something | Confirm pattern is gone: `! grep -rq '<pattern>' <dir>` |
| Security fix | Confirm bad pattern is gone: `! grep -rq '<vuln>' src/` |

### Rules

1. **Be specific** — `cargo test auth::refresh` not `cargo test`
2. **Be deterministic** — no manual checks
3. **Match the task** — prove THIS task is done
4. **Chain when needed** — `cargo test auth && cargo clippy`

## Fail-First TDD

On by default. When you create a bean with `--verify`:

1. Verify runs immediately → must **fail** (proving the test is real)
2. If it already passes → bean **rejected** (test doesn't test anything new)
3. After your work → `bn close` runs verify → must **pass**

Use `-p` / `--pass-ok` to skip fail-first for refactors, builds, docs — anything where there's no "before" failure state.

## Working on a Bean

### Check Progress
```bash
bn verify <id>                    # Run verify without closing
bn update <id> --note "progress"  # Log what you've done
```

### Close It
```bash
bn close <id>                    # Runs verify → closes if exit 0
bn close <id> --reason "summary" # With completion note
```

### If Stuck
```bash
bn update <id> --note "blocked: <why>"
bn claim <id> --release          # Release for another agent to retry
```

Notes are timestamped and visible to the next agent — they're the handoff protocol.

## Discovering Issues

While working, you'll notice problems that aren't your current task. **Don't fix them. Create a bean.**

```bash
bn create "bug: logger crashes on unicode" --verify "cargo test logger::unicode"
bn create "test: no coverage for cache" --verify "cargo test cache::"
bn create "docs: README missing setup" --verify "grep -q '## Setup' README.md"
bn create "security: API key in logs" --verify "! grep -r 'api_key.*log' src/"
```

Don't claim these — stay focused on your current work. Another agent handles them later.

**Type prefixes**: `bug:`, `test:`, `docs:`, `refactor:`, `perf:`, `security:`, `chore:`

## Delegating Work

Use `bn run` to dispatch beans to agents. Configure the agent command once:

```bash
bn config set run "claude -p 'implement bean {id} and run bn close {id}'"
```

Then dispatch:

```bash
bn run                  # Dispatch all ready beans
bn run 3                # Dispatch a specific bean
bn run --watch          # Continuous mode: re-dispatch as beans complete
```

For large beans that need decomposition:

```bash
bn plan 3               # Break bean 3 into children
bn plan --auto          # Autonomous (no prompts)
```

Monitor agents:

```bash
bn agents               # Show running/completed agents
bn logs 3               # View agent output for bean 3
```

| Approach | Who works | Blocks you? |
|----------|-----------|-------------|
| `--claim` / `bn quick` | You, now | Yes |
| `bn run` | Background agents | No |
| `bn run --watch` | Continuous dispatch | No |

## Decomposition

When a task is too big for one agent, create a parent and break it into children:

```bash
bn create "feat: auth system" --description "Parent for auth work"
bn create "Define types" --parent @latest --verify "cargo build" \
  --produces "AuthProvider,AuthConfig"
bn create "Implement JWT" --parent <id> --verify "cargo test jwt" \
  --requires "AuthProvider" --produces "JwtProvider"
bn create "Integration tests" --parent <id> --verify "cargo test auth::integration" \
  --requires "JwtProvider"
```

Children auto-number (`<parent>.1`, `<parent>.2`, ...). Use `produces`/`requires` for automatic dependency resolution — a bean requiring `AuthProvider` is blocked until the bean producing it closes.

View the hierarchy: `bn tree <id>`

## Pipe-Friendly Output

Use `--json` for structured output when composing commands:

```bash
# Create and capture ID
ID=$(bn create "task" --verify "test" -p --json | jq -r '.id')

# List just IDs (one per line)
bn list --ids

# Custom format
bn list --format '{id}\t{status}\t{title}'

# Batch close from pipe
bn list --ids | bn close --stdin --force

# Read description from stdin
cat spec.md | bn create "task" --description - --verify "test"

# Pipe build output into notes
cargo build 2>&1 | bn update 3 --notes -

# Structured verify result
bn verify 3 --json   # {"id":"3","passed":false}

# Context as JSON
bn context 3 --json  # {"id":"3","files":[{"path":"...","content":"..."}]}
```

## Command Reference

| Command | Purpose |
|---------|---------|
| `bn create "..." --verify "..."` | **Create a bean** (`--json` for piped output) |
| `bn quick "..." --verify "..."` | Create + claim in one step |
| `bn status` | Overview: claimed, ready, blocked |
| `bn ready` | Unblocked tasks |
| `bn claim <id>` | Claim a task |
| `bn show <id>` | Full bean details (the prompt) |
| `bn context <id>` | Files referenced in description (`--json`) |
| `bn verify <id>` | Test verify without closing (`--json`) |
| `bn close <id>` | Close (verify must pass, `--stdin` for batch) |
| `bn close <id> --force` | Force close (skip verify) |
| `bn update <id> --note "..."` | Log progress (`--description -` reads stdin) |
| `bn claim <id> --release` | Release claim |
| `bn run [id] [-j N]` | Dispatch ready beans to agents |
| `bn run --watch` | Watch mode: auto-dispatch on changes |
| `bn plan [id] [--auto]` | Decompose a large bean into children |
| `bn agents` | Show running/completed agents |
| `bn logs <id>` | View agent output for a bean |
| `bn tree` / `bn tree <id>` | View hierarchy |
| `bn list` | List beans (`--ids`, `--format`, `--json`) |
| `bn dep add <id> <dep>` | Add explicit dependency |
| `bn tidy` | Archive closed, release stale claims |
| `bn doctor` | Health check |

### Selectors

Use instead of numeric IDs: `@latest`, `@ready`, `@blocked`, `@me`

## The Verify Gate

**You cannot close without proof.** If verify fails, the task stays open, the attempt counter increments, and another agent can retry. This prevents incomplete work from slipping through.
