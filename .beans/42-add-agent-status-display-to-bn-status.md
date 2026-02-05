id: '42'
title: Add agent status display to bn status
slug: add-agent-status-display-to-bn-status
status: open
priority: 2
created_at: 2026-02-05T18:51:39.600639Z
updated_at: 2026-02-05T18:51:39.600639Z
description: "\n## Summary\nDisplay spro agent liveness in bn status output.\n\n## Changes\n- Add claimed_by field to IndexEntry (index.rs)\n- Parse spro:PID format from claimed_by\n- Check PID liveness using kill -0\n- Display indicators: ● (running) or ✗ (dead)\n- Include agent status in JSON output\n\n## Files\n- src/index.rs\n- src/commands/status.rs\n- src/commands/list.rs (test fixes)\n- src/selector.rs (test fixes)\n\n## Usage\nWhen spro claims a bean with `bn claim <id> --by spro:12345`:\n- bn status shows: `1.1 [-] Task (spro:12345 â\x97\x8F)`\n- JSON includes: `{ \"agent\": { \"pid\": 12345, \"alive\": true } }`\n"
verify: cargo test
tokens: 19956
tokens_updated: 2026-02-05T18:51:39.603792Z
