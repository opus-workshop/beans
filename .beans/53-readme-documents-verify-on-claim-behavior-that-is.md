id: '53'
title: README documents verify-on-claim behavior that is not implemented
slug: readme-documents-verify-on-claim-behavior-that-is
status: open
priority: 1
created_at: 2026-02-18T07:05:52.500558Z
updated_at: 2026-02-18T07:05:52.500558Z
description: |-
  **Problem:** The README describes verify-on-claim as a working feature in two places:

  **"For Agents" section (near top):**
  ```
  bn claim 3             # Verify runs. Must FAIL. (Proves work is needed.)
  ```

  **"Option 2: Automatic on claim" section:**
  ```
  bn claim 5
  # → Runs verify automatically
  # → Must FAIL to prove test is real
  ```

  But `cmd_claim` in `src/commands/claim.rs` does NOT run the verify command. It only checks status, token limits, and sets the bean to in_progress. Bean 11 ("Verify-on-claim") is still listed as in-progress in `bn status`, confirming this feature is not yet implemented.

  **Impact:** Users/agents reading the README will expect verify to run on claim, but it doesn't. The `--fail-first` flag on create/quick is the only enforced TDD mechanism.

  **Fix:** Either:
  1. Remove/mark the verify-on-claim sections as "planned" until bean 11 is complete
  2. Or implement verify-on-claim (bean 11)

  At minimum, the README should not document unimplemented behavior as working.

  **Files:**
  - `README.md` (search for "bn claim 3" and "Option 2: Automatic on claim")
acceptance: README accurately reflects the implemented behavior of bn claim (no verify-on-claim documented as working unless the feature is actually implemented)
tokens: 7236
tokens_updated: 2026-02-18T07:05:52.504614Z
