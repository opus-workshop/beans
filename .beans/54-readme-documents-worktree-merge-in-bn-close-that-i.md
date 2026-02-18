id: '54'
title: README documents worktree merge in bn close that is not implemented
slug: readme-documents-worktree-merge-in-bn-close-that-i
status: open
priority: 1
created_at: 2026-02-18T07:05:52.538493Z
updated_at: 2026-02-18T07:05:52.538493Z
description: "**Problem:** The README describes a worktree merge flow for `bn close` as if it's implemented:\n\n```\nbn close 5 (from .spro/5/)\n    │\n    ├── 1. Run verify → must pass\n    ├── 2. Commit changes in worktree\n    ├── 3. Merge to main branch\n    │       ├── Clean merge → continue\n    │       └── Conflict → fail, agent resolves, retries\n    ├── 4. Archive bean\n    └── 5. Remove worktree\n```\n\nBut `cmd_close` in `src/commands/close.rs` has **zero worktree-related code** — no calls to `commit_worktree_changes`, `merge_to_main`, or `cleanup_worktree`. The worktree functions exist in `src/worktree.rs` but are never called from close.\n\nThe related beans (12, 12.1, 12.3, 12.3.3) are still in-progress/blocked, confirming this integration is not complete.\n\n**Impact:** The README promises automatic worktree handling that doesn't exist, misleading agents/users about the close workflow.\n\n**Fix:** Either:\n1. Mark the worktree sections as \"planned\" / \"coming soon\" \n2. Or implement the worktree integration (beans 12.x)\n\n**Files:**\n- `README.md` (search for \"worktree\" — the close flow diagram and the parallel agents section)"
acceptance: README accurately reflects whether worktree merge is implemented in bn close. Unimplemented features are clearly marked as planned/upcoming.
tokens: 20468
tokens_updated: 2026-02-18T07:05:52.540200Z
