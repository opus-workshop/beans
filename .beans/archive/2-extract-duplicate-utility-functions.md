---
id: 2
title: Extract duplicate utility functions
status: open
priority: 2
created_at: |-
  2026-01-27T08:04:25.139443Z
updated_at: |-
  2026-01-27T08:04:25.139443Z
---

Eliminate copy-pasted natural_cmp, parse_id_segments, and parse_status logic across 3-4 locations.

## What to do
1. Create src/util.rs with centralized utilities
2. Move natural_cmp, parse_id_segments, implement FromStr for Status
3. Remove duplicates from commands/{list,ready,tree}.rs
4. Add comprehensive tests

## Acceptance
- cargo test passes
- Zero copy-paste instances remain
- Sorting behavior unchanged
