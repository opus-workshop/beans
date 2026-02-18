id: '55'
title: 'README Design Principle #2 says "No force-close" but --force flag exists'
slug: readme-design-principle-2-says-no-force-close-but
status: closed
priority: 2
created_at: 2026-02-18T07:05:52.556163Z
updated_at: 2026-02-18T08:03:00.181111Z
description: |-
  **Problem:** The README's Design Principles section states:

  > **2. Verify gates are mandatory.** No force-close. If you can't prove it's done, it's not done.

  But `bn close --force` exists and is implemented in `src/commands/close.rs`:

  ```rust
  // In cli.rs:
  /// Skip verify command (force close)
  #[arg(long)]
  force: bool,

  // In close.rs:
  if force {
      println!("Skipping verify for bean {} (--force)", id);
  }
  ```

  This is a direct contradiction between the documentation and the implementation.

  **Fix:** Either:
  1. Remove `--force` from close (breaking change, would match the design principle)
  2. Update Design Principle #2 to acknowledge `--force` exists as an escape hatch (recommended — there are legitimate reasons for force-close, like when verify depends on external services)

  **Files:**
  - `README.md` (Design Principles section, principle #2)
acceptance: 'Design Principle #2 in README accurately reflects the existence (or absence) of --force on bn close'
closed_at: 2026-02-18T08:03:00.181111Z
is_archived: true
tokens: 15917
tokens_updated: 2026-02-18T07:05:52.557371Z
