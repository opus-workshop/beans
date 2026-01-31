---
id: 6
title: Add path traversal protection for bean IDs
status: closed
priority: 0
created_at: |-
  2026-01-30T18:41:52.048039Z
updated_at: |-
  2026-01-30T18:51:07.517469Z
labels:
  - security
  - core
closed_at: |-
  2026-01-30T18:51:07.517469Z
verify: |-
  cargo test --lib util::validate
---

Validate bean IDs to prevent directory escape attacks.

Currently, bean IDs are used directly in path construction without validation:
  beans_dir.join(format!("{}.yaml", id))

A malformed ID like ../../../etc/passwd could potentially escape the .beans/ directory.

## Solution
Add a validate_bean_id() function that ensures IDs match safe pattern: ^[a-zA-Z0-9._-]+$

Call this validation in:
- src/lib.rs - ID parsing
- src/cli.rs - Early in command dispatch after parsing args  
- src/util.rs - Add the validation function here

## Acceptance Criteria
- Bean ID validation function exists in src/util.rs
- IDs validated before any file operation
- Invalid IDs rejected with clear error message
- All existing tests pass
- New test covers rejection of ../ and other invalid patterns
