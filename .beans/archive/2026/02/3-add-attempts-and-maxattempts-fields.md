id: '3'
title: Add attempts and max_attempts fields
status: closed
priority: 2
created_at: 2026-01-27T08:04:26.727807Z
updated_at: 2026-02-02T17:36:00.948969Z
description: |
  Add attempts and max_attempts fields to Bean struct. Required for agent retry workflow.

  ## What to do
  1. Add attempts: u32 and max_attempts: u32 to Bean struct
  2. Initialize attempts=0, max_attempts=3
  3. Update tests, add --max-attempts CLI flag

  ## Acceptance
  - cargo test passes
  - New beans have correct defaults
  - Fields round-trip YAML serialization
closed_at: 2026-02-02T17:36:00.948969Z
dependencies:
- '1'
is_archived: true
