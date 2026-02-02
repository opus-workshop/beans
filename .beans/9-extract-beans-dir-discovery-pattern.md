id: '9'
title: Extract beans_dir discovery pattern
status: closed
priority: 1
created_at: 2026-01-30T18:41:56.064554Z
updated_at: 2026-01-30T18:46:30.023197Z
description: |
  Refactor repetitive find_beans_dir() calls in src/main.rs.

  Currently lines 41-59+ repeat the same pattern across all command match arms:
    let cwd = env::current_dir()?;
    let beans_dir = find_beans_dir(&cwd)?;

  This pattern appears 18+ times.

  ## Solution
  1. Extract lookup to single point early in main(), before the match
  2. Pass beans_dir to command handler functions
  3. Only Init command doesn't need it — it creates the directory

  Result: Cleaner dispatch, ~36 fewer lines of repetition.

  ## Acceptance Criteria
  - Single find_beans_dir() call early in main (with Init exception)
  - All commands receive beans_dir from main
  - All tests pass without modification to test logic
  - Match arms visually cleaner (~36 fewer lines)
  - No functional change to command behavior
labels:
- refactor
- code-quality
dependencies:
- '3'
verify: cargo test --lib
