id: '15'
title: Remove attempt limit, append truncated failures to notes
slug: remove-attempt-limit-append-truncated-failures-to
status: closed
priority: 2
created_at: 2026-02-03T05:40:14.812058Z
updated_at: 2026-02-03T05:45:15.859772Z
description: |-
  Changes:
  1. Remove max_attempts limit (or make it effectively infinite)
  2. On verify failure in `bn close`, append formatted failure to notes:
     - First 50 lines + last 50 lines (or less if output is shorter)
     - Markdown format: `## Attempt N — timestamp` + exit code + fenced output
  3. Keep attempts counter for visibility, just don't enforce a limit

  Files to modify:
  - src/commands/close.rs (main logic)
  - src/bean.rs (maybe remove max_attempts default, or set to u32::MAX)
  - README.md (update docs)
closed_at: 2026-02-03T05:45:15.859772Z
close_reason: Removed max_attempts limit, failures now append to notes with first 50 + last 50 lines truncation
verify: cargo test
claimed_by: pi-agent
claimed_at: 2026-02-03T05:40:14.812057Z
is_archived: true
