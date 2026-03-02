---
id: '115'
title: Replace panic!() with proper error handling in non-test code
slug: replace-panic-with-proper-error-handling-in-non-te
status: closed
priority: 2
created_at: '2026-03-02T02:27:57.350964Z'
updated_at: '2026-03-02T02:30:21.703390Z'
labels:
- chore
- publish-prep
closed_at: '2026-03-02T02:30:21.703390Z'
verify: cd /Users/asher/beans && ! rg 'panic!\(' src/ -g '!*test*' --quiet
fail_first: true
checkpoint: '655ee759245f4c916a5cb00eff3a627366a732b7'
claimed_by: pi-agent
claimed_at: '2026-03-02T02:28:05.071506Z'
is_archived: true
tokens: 44441
tokens_updated: '2026-03-02T02:27:57.351954Z'
history:
- attempt: 1
  started_at: '2026-03-02T02:30:21.703885Z'
  finished_at: '2026-03-02T02:30:21.759685Z'
  duration_secs: 0.055
  result: pass
  exit_code: 0
attempt_log:
- num: 1
  outcome: success
  agent: pi-agent
  started_at: '2026-03-02T02:28:05.071506Z'
  finished_at: '2026-03-02T02:30:21.703390Z'
---

## Task
There are 3 `panic!()` calls in non-test production code. Replace them with proper error handling.

## Locations
1. **`src/worktree.rs`** — `panic!("Expected Conflict variant")` — replace with `unreachable!("Expected Conflict variant")` if this is truly an impossible code path in a match arm, or `anyhow::bail!()` if it can actually be reached.

2. **`src/commands/close.rs`** — `panic!("git {:?} failed to execute: {}", args, e)` — replace with `anyhow::bail!("git {:?} failed to execute: {}", args, e)` or return an `Err(...)`.

3. **`src/pi_output.rs`** — `panic!("expected ToolEnd, got {:?}", other)` — replace with `anyhow::bail!()` or `unreachable!()` depending on whether this case is reachable in practice.

## Steps
1. Read the context around each `panic!()` to understand if it's truly unreachable or an error case
2. Replace with appropriate alternative:
   - Truly unreachable match arms → `unreachable!()`
   - Error cases that can happen at runtime → `anyhow::bail!()` (may require changing return type to `Result`)
3. Run `cargo check` and `cargo test`

## Important
The verify command uses `rg` with `-g '!*test*'` to exclude test files. Make sure the `panic!()` calls in test code are NOT touched — only the 3 in production code.

## Don't
- Don't touch `panic!()` inside `#[cfg(test)]` modules — those are fine in tests
- Don't change `Bean::new()` which uses `.expect()` — that's a documented panicking convenience method
- Don't change function signatures unless necessary for the `bail!()` conversion
