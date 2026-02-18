id: '62'
title: 'on_fail routing: retry/run/escalate'
slug: onfail-routing-retryrunescalate
status: closed
priority: 1
created_at: 2026-02-22T07:45:24.963980Z
updated_at: 2026-02-22T08:47:52.832288Z
description: |-
  ## Goal
  Add declarative failure routing to beans, inspired by Visor's on_fail/on_success routing. When a bean's verify fails, instead of just incrementing attempts and waiting for manual retry, beans should be able to automatically take recovery actions.

  ## Motivation
  Currently when verify fails: attempts increments, failure output is appended to notes, claim is released. The next retry does the exact same thing. There's no way to say 'if this fails, try a different approach' or 'if this fails twice, escalate to a human'. Visor solves this with on_fail routing (retry with backoff, run remediation steps, goto earlier step, escalate).

  ## What to Build

  ### 1. OnFailAction enum
  ```rust
  pub enum OnFailAction {
      /// Retry the same bean (optionally with delay)
      Retry { max: Option<u32>, delay_secs: Option<u64> },
      /// Create and run a remediation bean
      Run { title: String, verify: Option<String>, description: Option<String> },
      /// Bump priority and optionally notify
      Escalate { priority: Option<u8>, message: Option<String> },
      /// Re-run a dependency (go back to earlier bean in the chain)
      Goto { bean_id: String },
  }
  ```

  ### 2. on_fail field on Bean
  - `pub on_fail: Option<OnFailAction>` on Bean struct
  - YAML serialization with serde tag
  - CLI: `--on-fail retry:3` or `--on-fail escalate:P0`

  ### 3. Process on_fail in close command
  - When verify fails in cmd_close, check bean.on_fail
  - Retry: if attempts < max, re-release the claim (existing behavior but now explicit)
  - Run: create a child bean with the remediation task, include failure output in description
  - Escalate: update priority, add note, optionally run notification command
  - Goto: re-open the target bean and release its claim

  ### 4. Integration with bw/deli
  - bw should respect on_fail when deciding what to do with failed beans
  - deli should process on_fail before moving to next wave

  ## Files
  - src/bean.rs (OnFailAction enum, on_fail field)
  - src/commands/close.rs (process on_fail when verify fails)
  - src/commands/create.rs (--on-fail CLI flag)
  - src/cli.rs (parse --on-fail argument)

  ## Edge Cases
  - on_fail Run should include failure output in the remediation bean description
  - Goto should check target bean exists and is a valid target (sibling or dependency)
  - Escalate should not go below P0
  - Retry with delay: who enforces the delay? (bw can wait, deli can sleep)
  - Circular goto detection: A goto B goto A → use max_attempts as circuit breaker
closed_at: 2026-02-22T08:47:52.832288Z
close_reason: 'Auto-closed: all children completed'
verify: cargo test on_fail
is_archived: true
tokens: 34370
tokens_updated: 2026-02-22T07:45:24.966848Z
