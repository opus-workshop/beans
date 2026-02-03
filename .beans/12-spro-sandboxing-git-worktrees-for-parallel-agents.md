id: '12'
title: 'Spro sandboxing: git worktrees for parallel agents'
slug: spro-sandboxing-git-worktrees-for-parallel-agents
status: open
priority: 2
created_at: 2026-02-03T03:21:51.875721Z
updated_at: 2026-02-03T03:21:51.875721Z
description: |-
  ## Summary
  Each spro-spawned agent gets an isolated git worktree.
  Prevents code conflicts between parallel agents.

  ## Flow
  1. spro run <parent> spawns agents for children
  2. For each child: git worktree add .spro/<id> HEAD
  3. Agent works in isolated worktree
  4. On close: merge worktree back to main
  5. Clean up: git worktree remove

  ## Why
  - Clean state for verify-on-claim
  - No interference between parallel agents
  - Easy rollback (delete worktree)
  - Atomic merges per bean

  ## Files
  - src/spro.rs (or pi extension)
  - Integration with verify-on-claim

  ## Questions
  - Merge strategy: rebase vs merge commit?
  - Conflict handling when merging back?
  - Non-git fallback?

  ## Depends on
  - Bean 11 (verify-on-claim)
dependencies:
- '11'
verify: cargo test spro::tests::worktree_isolation
