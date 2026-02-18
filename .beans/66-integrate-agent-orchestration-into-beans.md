id: '66'
title: Integrate agent orchestration into beans
slug: integrate-agent-orchestration-into-beans
status: closed
priority: 1
created_at: 2026-02-23T09:48:09.733923Z
updated_at: 2026-02-23T10:29:43.527101Z
closed_at: 2026-02-23T10:22:09.442392Z
close_reason: 'Auto-closed: all children completed'
verify: cargo test && bn run --help 2>&1 | grep -q 'Dispatch' && bn plan --help 2>&1 | grep -q 'plan' && bn agents --help 2>&1 | grep -q 'agents' && bn logs --help 2>&1 | grep -q 'logs'
tokens: 10
tokens_updated: 2026-02-23T09:48:09.736675Z
