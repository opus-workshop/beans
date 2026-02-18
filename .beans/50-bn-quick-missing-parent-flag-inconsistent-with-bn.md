id: '50'
title: bn quick missing --parent flag — inconsistent with bn create
slug: bn-quick-missing-parent-flag-inconsistent-with-bn
status: open
priority: 3
created_at: 2026-02-18T06:54:10.704890Z
updated_at: 2026-02-18T06:54:10.704890Z
description: |-
  **Problem:** The `quick` command (in `src/commands/quick.rs` and `src/cli.rs`) is documented as a convenience shortcut for `create + claim`, but it doesn't support the `--parent` flag. Users who want to create a child bean and immediately claim it must use `bn create --parent X --claim` instead.

  This creates an asymmetry:
  - `bn create "task" --parent 1 --claim --verify "test"` ✓ works
  - `bn quick "task" --parent 1 --verify "test"` ✗ no --parent flag

  **Fix:** Add `--parent` option to `QuickArgs` and the `Quick` CLI variant. In `cmd_quick`, use `assign_child_id()` when parent is specified (same logic as `cmd_create`).

  **Files:**
  - `src/cli.rs` (Quick command variant)
  - `src/commands/quick.rs` (QuickArgs struct, cmd_quick function)
verify: cd /Users/asher/beans && cargo build 2>&1 | grep -v warning | tail -1 | grep -q "Finished"
tokens: 6225
tokens_updated: 2026-02-18T06:54:10.706032Z
