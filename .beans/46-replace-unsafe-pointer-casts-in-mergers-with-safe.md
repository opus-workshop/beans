id: '46'
title: Replace unsafe pointer casts in merge.rs with safe field accessors
slug: replace-unsafe-pointer-casts-in-mergers-with-safe
status: in_progress
priority: 1
created_at: 2026-02-18T06:54:10.633956Z
updated_at: 2026-02-18T08:36:59.943295Z
description: |-
  **Problem:** `src/merge.rs` uses `unsafe` raw pointer casts in `get_field`, `set_field`, `get_field_option`, and `set_field_option` methods (14 occurrences). The pattern:

  ```rust
  let ptr = &self.status as *const _ as *const T;
  Ok(unsafe { (*ptr).clone() })
  ```

  This casts `&FieldType` to `*const T` where T is a generic parameter. If T doesn't match the actual field type, this is undefined behavior. The code currently works because all callers pass the correct types, but:

  1. There is no compile-time guarantee of type safety
  2. Any future refactoring could silently introduce UB
  3. This pattern is unnecessary — the same logic can be achieved safely

  **Fix:** Replace the generic+unsafe approach with direct field access via match arms that return/set the actual field values. For example, `merge_scalar` can use a macro or separate methods per field type, or use an enum to represent field values.

  **Files:**
  - `src/merge.rs` (lines 370-490)

  **Severity:** High — unsafe code with soundness risk in a critical merge path
verify: cd /Users/asher/beans && ! grep -q "unsafe" src/merge.rs && cargo test merge
claimed_at: 2026-02-18T08:36:59.943295Z
tokens: 7613
tokens_updated: 2026-02-18T06:54:10.638434Z
