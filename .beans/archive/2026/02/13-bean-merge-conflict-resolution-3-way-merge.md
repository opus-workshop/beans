id: '13'
title: Bean merge conflict resolution (3-way merge)
slug: bean-merge-conflict-resolution-3-way-merge
status: closed
priority: 2
created_at: 2026-02-03T03:22:00.802330Z
updated_at: 2026-02-03T07:45:34.297559Z
description: |-
  ## Summary
  Implement field-level 3-way merge for bean metadata conflicts.
  When worktrees merge back, bean files may conflict.

  ## From design doc
  See docs/design/CONFLICT_RESOLUTION.md

  ## Phases
  1. Version hash tracking (optimistic locking)
  2. Conflict detection (fail on any conflict)
  3. 3-way merge (auto-resolve non-overlapping)
  4. bn resolve command for manual resolution

  ## Files
  - src/bean.rs (hash, conflicts field)
  - src/merge.rs (new)
  - src/commands/resolve.rs (new)

  ## Depends on
  - Bean 12 (sandboxing - this handles merge-back conflicts)
closed_at: 2026-02-03T07:45:34.297559Z
close_reason: 'Auto-closed: all children completed'
dependencies:
- '12'
verify: cargo test merge::tests
is_archived: true
