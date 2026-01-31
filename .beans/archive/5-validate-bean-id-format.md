---
id: 5
title: Validate bean ID format
status: open
priority: 2
created_at: |-
  2026-01-27T08:04:29.994972Z
updated_at: |-
  2026-01-27T08:04:42.107985Z
dependencies: - '2'
---

Add validation that bean IDs follow pattern (digits and dots only). Prevents crashes from invalid manually-edited YAML.

## What to do
1. Add validate_bean_id function to util.rs
2. Call in Bean::from_file after deserializing
3. Support valid formats: '1', '3.2', '3.2.1'
4. Reject: '', 'a', '1.a', '1.', '.1'

## Acceptance
- cargo test passes
- Invalid IDs rejected with clear error
