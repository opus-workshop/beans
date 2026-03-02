---
id: '111'
title: Fix all clippy warnings
slug: fix-all-clippy-warnings
status: closed
priority: 1
created_at: '2026-03-02T02:27:57.145374Z'
updated_at: '2026-03-02T02:40:53.165638Z'
labels:
- chore
- publish-prep
closed_at: '2026-03-02T02:40:53.165638Z'
verify: cd /Users/asher/beans && cargo clippy --all-targets 2>&1 | grep -c "^warning:" | grep -q "^0$"
fail_first: true
checkpoint: '655ee759245f4c916a5cb00eff3a627366a732b7'
claimed_by: pi-agent
claimed_at: '2026-03-02T02:28:05.252209Z'
is_archived: true
tokens: 8007
tokens_updated: '2026-03-02T02:27:57.148308Z'
history:
- attempt: 1
  started_at: '2026-03-02T02:40:53.168155Z'
  finished_at: '2026-03-02T02:40:53.329752Z'
  duration_secs: 0.161
  result: pass
  exit_code: 0
attempt_log:
- num: 1
  outcome: success
  agent: pi-agent
  started_at: '2026-03-02T02:28:05.252209Z'
  finished_at: '2026-03-02T02:40:53.165638Z'
---

## Task
Fix all 39 clippy warnings in the codebase. These break CI which runs `cargo clippy -- -D warnings`.

## Warning categories
1. **27× "the borrowed expression implements the required traits"** — passing `&String` where `&str` suffices, or `&PathBuf` where `&Path` works. Remove unnecessary borrows.
2. **8× `assert_eq!` with literal bool** — in `src/hooks.rs` tests. Replace `assert_eq!(x, true)` with `assert!(x)` and `assert_eq!(x, false)` with `assert!(!x)`.
3. **2× useless `format!`** — replace `format!("...")` with string literal where no interpolation is needed.
4. **2× `is_none()` after `find()`** — use `!iter.any(...)` instead of `iter.find(...).is_none()`.
5. **1× assertion is always true** — find and fix or remove.

## Steps
1. Run `cargo clippy --all-targets 2>&1` to see all warnings with file locations
2. Fix each category systematically
3. Run `cargo clippy --all-targets 2>&1 | grep "^warning:"` and verify 0 warnings remain
4. Run `cargo test` to make sure nothing broke

## Don't
- Don't change any logic or behavior — these are purely cosmetic fixes
- Don't suppress warnings with `#[allow(...)]` — fix the actual code
- Don't touch code that isn't flagged by clippy
