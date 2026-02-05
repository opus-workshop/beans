id: '35'
title: 'spro: skip beans without verify (GOALs) in wave computation'
slug: spro-skip-beans-without-verify-goals-in-wave-compu
status: closed
priority: 2
created_at: 2026-02-05T08:36:29.316197Z
updated_at: 2026-02-05T08:37:17.762039Z
description: "## Context\n\nbeans now enforces: no verify = GOAL (needs decomposition), has verify = SPEC (ready for work).\n\nspro's `compute_waves` currently only filters by `status == \"open\"`. It should also filter out beans without verify commands.\n\n## Files\n- /Users/asher/spro/src/beans.rs (compute_waves function)\n\n## Contract\n\nUpdate `compute_waves` to:\n1. Only include beans where `verify.is_some()` \n2. Emit warning for beans without verify: \"Skipping GOAL {id} (no verify) - decompose first\"\n\n## Current (beans.rs line ~130)\n```rust\nlet mut remaining: Vec<Bean> = beans\n    .iter()\n    .filter(|b| b.status == \"open\")\n    .cloned()\n    .collect();\n```\n\n## New\n```rust\nlet (specs, goals): (Vec<_>, Vec<_>) = beans\n    .iter()\n    .filter(|b| b.status == \"open\")\n    .partition(|b| b.verify.is_some());\n\nif !goals.is_empty() {\n    eprintln!(\"Skipping {} GOALs (no verify) - decompose first:\", goals.len());\n    for b in &goals {\n        eprintln!(\"  {} {}\", b.id, b.title);\n    }\n}\n\nlet mut remaining: Vec<Bean> = specs.into_iter().cloned().collect();\n```"
notes: |2

  ## Attempt 1 — 2026-02-05T08:37:07Z
  Exit code: 101

  ```
  Compiling spro v0.1.0 (/Users/asher/spro)
  error[E0432]: unresolved import `tempfile`
     --> src/worktree.rs:248:9
      |
  248 |     use tempfile::TempDir;
      |         ^^^^^^^^ use of unresolved module or unlinked crate `tempfile`
      |
      = help: if you wanted to use a crate named `tempfile`, use `cargo add tempfile` to add it to your `Cargo.toml`

  warning: unused imports: `Worktree` and `self`
    --> src/spawn.rs:16:23
     |
  16 | use crate::worktree::{self, Worktree};
     |                       ^^^^  ^^^^^^^^
     |
     = note: `#[warn(unused_imports)]` (part of `#[warn(unused)]`) on by default

  warning: unused variable: `success`
     --> src/live.rs:175:43
      |
  175 |                 let (done, running_count, success, failed) = count_status(&beans_map);
      |                                           ^^^^^^^ help: if this is intentional, prefix it with an underscore: `_success`
      |
      = note: `#[warn(unused_variables)]` (part of `#[warn(unused)]`) on by default

  warning: unused variable: `failed`
     --> src/live.rs:175:52
      |
  175 |                 let (done, running_count, success, failed) = count_status(&beans_map);
      |                                                    ^^^^^^ help: if this is intentional, prefix it with an underscore: `_failed`

  For more information about this error, try `rustc --explain E0432`.
  warning: `spro` (bin "spro" test) generated 3 warnings
  error: could not compile `spro` (bin "spro" test) due to 1 previous error; 3 warnings emitted
  ```
closed_at: 2026-02-05T08:37:17.762039Z
close_reason: Updated compute_waves to filter out GOALs (beans without verify). Added partition logic to separate SPECs from GOALs, with warning output for skipped GOALs. Added test_compute_waves_skips_goals test. Also added tempfile dev-dependency.
verify: cd /Users/asher/spro && cargo test compute_waves
attempts: 1
claimed_at: 2026-02-05T08:36:29.384853Z
is_archived: true
tokens: 2072
tokens_updated: 2026-02-05T08:36:29.319640Z
