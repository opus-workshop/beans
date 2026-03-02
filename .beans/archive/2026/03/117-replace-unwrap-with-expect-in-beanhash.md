---
id: '117'
title: Replace unwrap() with expect() in Bean::hash()
slug: replace-unwrap-with-expect-in-beanhash
status: closed
priority: 3
created_at: '2026-03-02T02:27:57.393432Z'
updated_at: '2026-03-02T02:30:18.626460Z'
labels:
- chore
- publish-prep
closed_at: '2026-03-02T02:30:18.626460Z'
verify: cd /Users/asher/beans && ! sed -n '1,/^#\[cfg(test)\]/p' src/bean.rs | grep -q '\.unwrap()'
fail_first: true
checkpoint: '655ee759245f4c916a5cb00eff3a627366a732b7'
claimed_by: pi-agent
claimed_at: '2026-03-02T02:28:05.087034Z'
is_archived: true
tokens: 13846
tokens_updated: '2026-03-02T02:27:57.394636Z'
history:
- attempt: 1
  started_at: '2026-03-02T02:30:18.626817Z'
  finished_at: '2026-03-02T02:30:18.679931Z'
  duration_secs: 0.053
  result: pass
  exit_code: 0
attempt_log:
- num: 1
  outcome: success
  agent: pi-agent
  started_at: '2026-03-02T02:28:05.087034Z'
  finished_at: '2026-03-02T02:30:18.626460Z'
---

## Task
In `src/bean.rs`, the `Bean::hash()` method at line ~498 uses `.unwrap()`:

```rust
let json = serde_json::to_string(&canonical).unwrap();
```

This can't realistically fail (serializing a struct that derives Serialize), but `expect()` is more self-documenting for a library crate.

## Steps
1. Open `src/bean.rs`
2. Find the `hash()` method (around line 498)
3. Change `.unwrap()` to `.expect("Bean serialization to JSON cannot fail")`
4. Check if there are any other `.unwrap()` calls in non-test code in this file and fix those too
5. Run `cargo test`

## Don't
- Don't touch `.unwrap()` calls inside `#[cfg(test)]` mod tests — those are fine
- Don't change any logic
