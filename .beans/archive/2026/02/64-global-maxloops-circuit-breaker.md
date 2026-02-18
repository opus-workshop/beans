id: '64'
title: Global max_loops circuit breaker
slug: global-maxloops-circuit-breaker
status: closed
priority: 2
created_at: 2026-02-22T07:45:54.917241Z
updated_at: 2026-02-22T09:07:24.613707Z
description: "## Goal\nAdd a global circuit breaker that prevents runaway retry/decompose cascades across a bean subtree. Inspired by Visor's routing.max_loops.\n\n## Motivation  \nCurrently each bean has max_attempts (default 3). But there's no limit on the TOTAL failures across a parent's subtree. If bean 14.1 fails 3 times, gets re-decomposed into 14.1.1-14.1.3, and those each fail 3 times, that's 12 agent runs with no global stop. With on_fail routing (bean 62), this could get worse — remediation beans spawning more remediation beans.\n\n## What to Build\n\n### 1. max_loops config\n- Add `max_loops: u32` to Config (default: 10)  \n- Add `max_loops: Option<u32>` to Bean (override per-bean)\n- CLI: `bn config set max_loops 20`\n\n### 2. Loop counting\n- Track total verify attempts across a subtree\n- On each verify attempt, walk up to root parent, sum all descendant attempts\n- If sum >= max_loops, refuse to retry — mark bean as `escalated` or add label\n\n### 3. Escalation behavior\n- When max_loops hit: set bean priority to P0, add label `circuit-breaker`\n- Print warning: 'Bean {id} subtree exceeded max_loops ({n}), escalating'\n- Do NOT auto-retry or auto-decompose further\n\n### 4. bn status integration\n- Show circuit-breaker beans in status output\n- `bn status` should flag subtrees that are close to max_loops\n\n## Files\n- src/config.rs (max_loops field)\n- src/bean.rs (optional max_loops override)\n- src/commands/close.rs (check loop count before retry)\n- src/commands/status.rs (display circuit-breaker warnings)\n- src/graph.rs (subtree attempt counting)\n\n## Edge Cases\n- Archived/closed beans should not count toward loop total\n- max_loops of 0 means unlimited (disable circuit breaker)\n- Per-bean max_loops overrides global config\n- What counts as a 'loop'? Just verify attempts, or also decomposition events?"
closed_at: 2026-02-22T09:07:24.613707Z
close_reason: 'Auto-closed: all children completed'
verify: cargo test max_loops
is_archived: true
tokens: 25514
tokens_updated: 2026-02-22T07:45:54.920029Z
