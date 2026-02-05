id: '32'
title: 'Enforce verify-as-spec: beans without verify are goals, not ready'
slug: enforce-verify-as-spec-beans-without-verify-are-go
status: closed
priority: 2
created_at: 2026-02-05T08:14:28.190389Z
updated_at: 2026-02-05T08:20:15.531498Z
description: "## Goal\n\nMake the Goal → Spec → Test hierarchy enforced by tooling:\n- A bean without a verify command is a GOAL (needs clarification)\n- A bean with a verify command is a SPEC (ready for work)\n- `bn ready` only shows SPECs (has verify + no blocking deps)\n\n## Files\n- src/commands/ready.rs\n- src/commands/status.rs  \n- src/commands/create.rs (--run requires --verify)\n- src/commands/claim.rs (warn if no verify)\n\n## Why\nThe verify command IS the spec. If you can't write one, you don't know what \"done\" looks like."
closed_at: 2026-02-05T08:20:15.531498Z
close_reason: 'Auto-closed: all children completed'
verify: cargo test
is_archived: true
tokens: 14848
tokens_updated: 2026-02-05T08:14:28.192426Z
