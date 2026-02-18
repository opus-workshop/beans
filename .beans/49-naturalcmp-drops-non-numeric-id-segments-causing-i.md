id: '49'
title: natural_cmp drops non-numeric ID segments, causing incorrect sort for alpha IDs
slug: naturalcmp-drops-non-numeric-id-segments-causing-i
status: open
priority: 3
created_at: 2026-02-18T06:54:10.690759Z
updated_at: 2026-02-18T08:36:18.515702Z
description: "**Problem:** In `src/util.rs`, `parse_id_segments()` silently drops non-numeric segments via `filter_map(|seg| seg.parse::<u64>().ok())`. Since `validate_bean_id()` accepts alphanumeric IDs like \"my-task\", \"ABC1\", and \"task_v1\", these IDs will be parsed as empty vectors or partial matches:\n\n- `parse_id_segments(\"my-task\")` → `[]` (all segments non-numeric, \"my-task\" can't split on '.')\n- `parse_id_segments(\"ABC1\")` → `[]`  \n- `parse_id_segments(\"task.v1\")` → `[]` (both \"task\" and \"v1\" fail u64 parse)\n\nThis means:\n- All alpha-only IDs compare as equal (empty vec == empty vec)\n- Sorting is non-deterministic for alpha IDs\n- Mixed alpha/numeric IDs like \"1.abc.2\" sort as [1, 2], losing ordering info\n\n**Fix:** Implement proper natural sort that handles mixed alpha-numeric segments:\n- Split on '.' \n- Compare segments: numeric segments compared numerically, alpha segments compared lexicographically\n\n**Files:**\n- `src/util.rs` (`parse_id_segments`, `natural_cmp`)\n- `src/index.rs` (uses `natural_cmp` for sorting index entries)"
verify: cd /Users/asher/beans && cargo test util::tests::natural_cmp
claimed_at: 2026-02-18T08:36:07.464977Z
tokens: 9946
tokens_updated: 2026-02-18T06:54:10.692072Z
