---
id: 4
title: Refactor graph functions to accept Index
status: closed
priority: 2
created_at: |-
  2026-01-27T08:04:28.350601Z
updated_at: |-
  2026-01-27T08:04:28.350601Z
---

Make graph.rs functions accept &Index instead of &Path. Eliminates redundant disk I/O and makes dependencies explicit.

## What to do
1. Change function signatures in graph.rs
2. Remove internal Index::load_or_rebuild calls
3. Update callers in commands/{dep,doctor,graph}.rs

## Acceptance
- cargo test passes
- Graph functions accept Index parameter
- Each command loads index once
- Existing behavior unchanged
