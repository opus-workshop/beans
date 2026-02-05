id: '39'
title: 'spro: unify spawn/pull/run into single smart command'
slug: spro-unify-spawnpullrun-into-single-smart-command
status: closed
priority: 2
created_at: 2026-02-05T10:28:49.327435Z
updated_at: 2026-02-05T10:31:12.915339Z
description: "## Contract\n\nMerge `spro spawn`, `spro pull`, and `spro run` into one smart command:\n\n```bash\nspro <id>              # smart: leaf bean or parent's children, background\nspro <id> --wait       # blocking (wait for completion)\nspro <id> -j 8         # parallelism for children\nspro <id> --dry-run    # preview what would happen\n```\n\n## Behavior\n\n1. **Detect bean type:**\n   - Has children with verify → run children (like current `pull`/`run`)\n   - No children (leaf) → run that bean (like current `spawn`)\n\n2. **Default: background (fire-and-forget)**\n   - Returns immediately\n   - Use `--wait` to block until completion\n\n3. **Flags:**\n   - `--wait` - block until done\n   - `-j N` / `--parallel N` - max parallel agents (default 4)\n   - `--dry-run` - show plan without executing\n   - `--keep-going` - continue if some fail\n   - `--timeout` / `--idle-timeout` - existing timeout flags\n\n## Examples\n\n```bash\nspro 32              # Run all children of 32 in background\nspro 32.1            # Run single bean 32.1 in background  \nspro 32 --wait       # Run children, wait for completion\nspro 32 -j 8 --wait  # Run children with 8 parallel, wait\nspro 32 --dry-run    # Show what would run\n```\n\n## Files\n- /Users/asher/spro/src/main.rs (CLI restructure)\n- /Users/asher/spro/src/pull.rs (merge with spawn logic)\n- /Users/asher/spro/src/spawn.rs (merge into unified runner)\n\n## Migration\n- Keep `spawn`, `pull`, `run` as hidden aliases for compatibility\n- Or deprecate with warning pointing to new syntax"
closed_at: 2026-02-05T10:31:12.915339Z
close_reason: Unified spro spawn/pull/run into single smart command. Now `spro <id>` auto-detects leaf vs parent beans, defaults to background (fire-and-forget), with `--wait` for blocking mode. Subcommands spawn/pull/run removed; status and logs remain.
verify: cd /Users/asher/spro && cargo build && ./target/debug/spro --help | grep -v "spawn\|pull\|run"
claimed_at: 2026-02-05T10:28:49.370872Z
is_archived: true
tokens: 8180
tokens_updated: 2026-02-05T10:28:49.329793Z
