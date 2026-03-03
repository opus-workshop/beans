---
name: beans
description: >
  Task tracker and agent orchestrator for AI coding. Verified gates, dependency scheduling, multi-agent
  dispatch. Create beans to delegate work — `bn run` dispatches agents automatically.
  Default action: `bn create "task" --verify "cmd"` (don't claim — let orchestration handle it).
---

# Beans — Quick Reference

Beans is a task tracker for AI agents where every task has a **verify gate** — a shell command that must exit 0 to close. `bn run` dispatches ready beans to agents, tracks failures, and re-dispatches as dependencies resolve.

**For syntax and examples:** `bn --help` or `bn <command> --help`

## When to Create

- Bug found while working → `bn create "bug: ..." --verify "test"`
- Multi-step feature → `bn create "feat: ..." --verify "test"`
- Tests needed → `bn create "test: ..." --verify "test"`
- Refactor/docs/chore → `bn create "refactor: ..." --verify "cmd" -p`

Use `--paths` to specify which files a bean touches (used by `bn context`):
```bash
bn create "fix auth" --verify "cargo test auth" --paths "src/auth.rs,src/routes.rs"
```

Don't claim — `bn run` dispatches agents. Use `-p` when verify already passes.

**Don't create** for questions, lookups, or trivial one-line fixes.

## Agent Context

`bn context <id>` is the single source of truth for agents. It outputs:
1. Bean spec (title, verify, description, acceptance)
2. Previous attempt notes (what was tried, what failed)
3. Project rules (RULES.md)
4. Dependency context (sibling beans that produce required artifacts)
5. Referenced file contents (from `paths` field + description text)

## Writing Good Descriptions

Bean descriptions are agent prompts. Quality determines agent success.

**Include:**
1. **Concrete steps** — numbered, actionable ("Add test for X in Y" not "test things")
2. **File paths with intent** — `src/auth.rs (modify — add validation)`
3. **Embedded context** — paste actual types/signatures the agent needs
4. **Acceptance criteria** — what "done" looks like beyond the verify command
5. **Anti-patterns** — what NOT to do (learned from previous failures)

**Example:**

```bash
bn create "Add expired token test" \
  --verify "cargo test auth::tests::test_expired" \
  --description "## Task
Add a test that verifies expired JWT tokens return 401.

## Steps
1. Open src/auth/tests/jwt_test.rs
2. Add test_expired_token_returns_401 using create_test_token() from fixtures
3. Set expiry to 1 hour ago, assert 401 response

## Context
\`\`\`rust
// from src/auth/token.rs
pub struct AuthToken {
    pub user_id: UserId,
    pub expires_at: DateTime<Utc>,
}
\`\`\`

## Files
- src/auth/tests/jwt_test.rs (modify)
- src/auth/tests/fixtures.rs (read — has create_test_token)
- src/auth/token.rs (read only — do NOT modify)

## Don't
- Don't modify AuthToken or add dependencies
- Don't change existing tests"
```

## On Failure

**Never retry with identical instructions.** Add what went wrong via `bn update <id> --note "..."`.

If an agent fails twice, the bean is too big or underspecified — `bn plan <id>` to break it down.
