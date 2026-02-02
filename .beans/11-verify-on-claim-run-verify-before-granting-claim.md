id: '11'
title: 'Verify-on-claim: run verify before granting claim'
slug: verify-on-claim-run-verify-before-granting-claim
status: in_progress
priority: 2
created_at: 2026-02-03T03:21:42.823080Z
updated_at: 2026-02-03T05:10:47.689160Z
description: "## Summary\nWhen claiming a bean with a verify command, run verify FIRST.\n- PASSES → reject claim (nothing to do or test is bogus)  \n- FAILS → grant claim, record checkpoint, set fail_first: true\n\n## Why\nEnforces TDD automatically. Checkpoint proves test was meaningful.\n\n## Files\n- src/commands/claim.rs\n- src/bean.rs (add checkpoint field)\n\n## Acceptance\n- bn claim with passing verify rejected\n- bn claim with failing verify succeeds\n- bn claim --force overrides\n- checkpoint SHA stored in bean"
verify: cargo test claim::tests::verify_on_claim
claimed_at: 2026-02-03T05:10:47.689160Z
