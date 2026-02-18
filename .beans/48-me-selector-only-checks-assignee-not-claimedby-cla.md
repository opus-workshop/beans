id: '48'
title: '@me selector only checks assignee, not claimed_by — claimed beans invisible'
slug: me-selector-only-checks-assignee-not-claimedby-cla
status: open
priority: 2
created_at: 2026-02-18T06:54:10.676689Z
updated_at: 2026-02-18T06:54:10.676689Z
description: |-
  **Problem:** In `src/selector.rs`, the `resolve_me()` function filters beans by `entry.assignee`, but the `claim` system sets `claimed_by` — a different field. This means:

  1. Agent runs `bn claim 1 --by agent-1` → sets `claimed_by: agent-1`
  2. Agent runs `bn show @me` → returns empty (because `assignee` is still None)

  The `assignee` field is set via `--assignee` on create/update, while `claimed_by` is set by the claim system. These are conceptually different (assignee = who should do it, claimed_by = who is doing it), but `@me` only checking assignee makes it useless for agents that claim beans.

  **Fix:** `resolve_me` should check both `assignee` and `claimed_by`:

  ```rust
  .filter(|entry| {
      (entry.assignee.as_ref() == Some(&current_user)
       || entry.claimed_by.as_ref() == Some(&current_user))
      && entry.status != Status::Closed
  })
  ```

  **Files:**
  - `src/selector.rs` (lines ~226-233, `resolve_me` function)
verify: cd /Users/asher/beans && cargo test selector::tests::resolve_me
tokens: 10048
tokens_updated: 2026-02-18T06:54:10.677929Z
