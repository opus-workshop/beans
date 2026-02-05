id: '37'
title: 'spro spawn: reject beans without verify (GOALs)'
slug: spro-spawn-reject-beans-without-verify-goals
status: closed
priority: 2
created_at: 2026-02-05T08:37:13.979480Z
updated_at: 2026-02-05T08:37:41.341071Z
description: |-
  ## Context

  beans enforces: no verify = GOAL, has verify = SPEC.
  pi's bn_create with run=true creates bean then calls spro spawn.
  If bean has no verify, spro spawn should reject it.

  ## Contract

  In `spro spawn`, check if bean has verify:
  ```rust
  let bean = beans::get_bean(id)?;
  if bean.verify.is_none() {
      anyhow::bail!(
          "Cannot spawn agent for bean {}: no verify command\n\n\
           This is a GOAL, not a SPEC. Decompose it into specs with verify commands first."
      );
  }
  ```

  ## File
  - /Users/asher/spro/src/spawn.rs (or wherever spawn logic lives)
closed_at: 2026-02-05T08:37:41.341071Z
close_reason: Added check in spawn_single to reject beans without verify commands (GOALs), with appropriate error message for both TUI and JSON stream modes.
verify: cd /Users/asher/spro && cargo build
claimed_at: 2026-02-05T08:37:14.024171Z
is_archived: true
tokens: 5235
tokens_updated: 2026-02-05T08:37:13.980540Z
