---
id: '114'
title: Restrict pub module visibility in lib.rs
slug: restrict-pub-module-visibility-in-librs
status: closed
priority: 1
created_at: '2026-03-02T02:27:57.328545Z'
updated_at: '2026-03-02T02:31:50.007421Z'
labels:
- chore
- publish-prep
closed_at: '2026-03-02T02:31:50.007421Z'
verify: cd /Users/asher/beans && grep -c '^pub(crate) mod' src/lib.rs | grep -qE '^[5-9]|^[1-9][0-9]' && cargo check 2>&1 | tail -1 | grep -q "Finished"
fail_first: true
checkpoint: '655ee759245f4c916a5cb00eff3a627366a732b7'
claimed_by: pi-agent
claimed_at: '2026-03-02T02:28:05.018576Z'
is_archived: true
tokens: 503
tokens_updated: '2026-03-02T02:27:57.329484Z'
history:
- attempt: 1
  started_at: '2026-03-02T02:31:50.009311Z'
  finished_at: '2026-03-02T02:31:50.113420Z'
  duration_secs: 0.104
  result: pass
  exit_code: 0
attempt_log:
- num: 1
  outcome: success
  agent: pi-agent
  started_at: '2026-03-02T02:28:05.018576Z'
  finished_at: '2026-03-02T02:31:50.007421Z'
---

## Task
Most modules in `src/lib.rs` are `pub mod` but should be `pub(crate) mod` — they're internal implementation details, not part of the library API. Publishing them as public means any signature change is a breaking change.

## Current state (src/lib.rs)
All 22 modules are `pub mod`. Only these should stay public (they form the library API surface):
- `api` — the structured API layer
- `bean` — core Bean type and related enums
- `config` — Config type
- `discovery` — find_beans_dir, find_bean_file, etc.
- `graph` — dependency graph functions
- `index` — Index type
- `util` — validate_bean_id, natural_cmp, etc.

## Modules to change to `pub(crate) mod`:
- `agent_presets`
- `commands`
- `ctx_assembler`
- `hooks`
- `locks`
- `mcp`
- `pi_output`
- `project`
- `relevance`
- `spawner`
- `stream`
- `timeout`
- `tokens`
- `worktree`
- `cli` (if it exists in lib.rs — check first, it might only be in main.rs)

That's 14-15 modules.

## Steps
1. Open `src/lib.rs`
2. Change the listed modules from `pub mod` to `pub(crate) mod`
3. Run `cargo check` to verify nothing outside the crate depends on them
4. Run `cargo test` to make sure tests still pass

## Don't
- Don't change `api`, `bean`, `config`, `discovery`, `graph`, `index`, `util` — these stay `pub`
- Don't modify any code inside the modules — just change visibility
- If `cargo check` fails because something in `main.rs` uses a now-restricted path, that's fine — `main.rs` is inside the crate and can access `pub(crate)` items. But if integration tests in `tests/` break, those need `pub` access — check and adjust.
