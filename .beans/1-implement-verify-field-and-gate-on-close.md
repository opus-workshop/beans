---
id: 1
title: Implement verify field and gate on close
status: closed
priority: 2
created_at: |-
  2026-01-27T08:04:24.135954Z
updated_at: |-
  2026-01-27T08:04:24.135954Z
---

Add the verify field to Bean struct and implement the verify-on-close lifecycle. This is the core missing feature that enables agent-driven workflows with automatic retry.

## What to do

1. Add `verify: Option<String>` field to Bean struct in src/bean.rs:34-66
2. Update serialization tests to handle the new field
3. Implement cmd_verify() in src/commands/verify.rs (new file)
4. Wire cmd_verify into src/main.rs dispatch
5. Update cmd_close() to run verify before closing
6. Add comprehensive tests

## Files
- src/bean.rs (add field + tests)
- src/commands/verify.rs (new)
- src/commands/close.rs (integrate verify gate)
- src/main.rs (dispatch)

## Acceptance
- cargo test passes
- bn close refuses to close if verify fails
- bn close succeeds if verify passes
- bn verify runs verify command and exits with correct code
