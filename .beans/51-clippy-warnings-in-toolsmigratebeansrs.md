id: '51'
title: Clippy warnings in tools/migrate_beans.rs
slug: clippy-warnings-in-toolsmigratebeansrs
status: open
priority: 4
created_at: 2026-02-18T06:54:10.717820Z
updated_at: 2026-02-18T06:54:10.717820Z
description: |-
  **Problem:** `cargo clippy` reports 5 warnings in `tools/migrate_beans.rs`, all `needless_borrows_for_generic_args`:

  1. Line 45: `.args(&[...])` → `.args([...])`
  2. Line 93: `.get(&Value::String(...))` → `.get(Value::String(...))`
  3. Line 101: `.get(&Value::String(...))` → `.get(Value::String(...))`
  4. Line 135: `.args(&[...])` → `.args([...])`
  5. Line 148: `.args(&[...])` → `.args([...])`

  **Fix:** Apply the suggestions from clippy. Can be auto-fixed with `cargo clippy --fix --bin migrate_beans`.

  **Files:**
  - `tools/migrate_beans.rs`
verify: cd /Users/asher/beans && cargo clippy 2>&1 | grep -c "warning" | grep -q "^0$"
tokens: 1895
tokens_updated: 2026-02-18T06:54:10.719002Z
