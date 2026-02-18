---
id: '49'
title: natural_cmp drops non-numeric ID segments, causing incorrect sort for alpha IDs
slug: naturalcmp-drops-non-numeric-id-segments-causing-i
status: closed
priority: 3
created_at: 2026-02-18T06:54:10.690759Z
updated_at: 2026-02-18T08:46:43.957385Z
closed_at: 2026-02-18T08:46:43.957385Z
verify: cd /Users/asher/beans && cargo test util::tests::natural_cmp
claimed_at: 2026-02-18T08:36:59.933155Z
is_archived: true
tokens: 9946
tokens_updated: 2026-02-18T06:54:10.692072Z
---

**Problem:** In `src/util.rs`, `parse_id_segments()` silently drops non-numeric segments via `filter_map(|seg| seg.parse::<u64>().ok())`. Since `validate_bean_id()` accepts alphanumeric IDs like "my-task", "ABC1", and "task_v1", these IDs will be parsed as empty vectors or partial matches:

- `parse_id_segments("my-task")` → `[]` (all segments non-numeric, "my-task" can't split on '.')
- `parse_id_segments("ABC1")` → `[]`  
- `parse_id_segments("task.v1")` → `[]` (both "task" and "v1" fail u64 parse)

This means:
- All alpha-only IDs compare as equal (empty vec == empty vec)
- Sorting is non-deterministic for alpha IDs
- Mixed alpha/numeric IDs like "1.abc.2" sort as [1, 2], losing ordering info

**Fix:** Implement proper natural sort that handles mixed alpha-numeric segments:
- Split on '.' 
- Compare segments: numeric segments compared numerically, alpha segments compared lexicographically

**Files:**
- `src/util.rs` (`parse_id_segments`, `natural_cmp`)
- `src/index.rs` (uses `natural_cmp` for sorting index entries)
