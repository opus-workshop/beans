id: '60'
title: post-close hook + on_close actions
slug: post-close-hook-onclose-actions
status: closed
priority: 1
created_at: 2026-02-22T07:44:50.210033Z
updated_at: 2026-02-22T08:29:45.189748Z
description: |-
  ## Goal
  Add a post-close hook event and declarative on_close actions to beans, inspired by Visor's event-driven routing.

  ## Motivation
  Currently beans has pre-create, post-create, pre-update, post-update, pre-close hooks but NO post-close. When a bean closes, nothing happens automatically. This limits workflow chaining — you can't trigger follow-up work, notifications, or downstream verification.

  ## What to Build

  ### 1. PostClose hook event
  - Add `PostClose` variant to `HookEvent` enum in src/hooks.rs
  - Fire it after successful close in src/commands/close.rs
  - Hook script receives the closed bean as JSON payload (same pattern as other hooks)

  ### 2. Declarative on_close field on Bean
  - Add `on_close: Vec<OnCloseAction>` to Bean struct in src/bean.rs
  - Actions:
    - `run: "shell command"` — execute a shell command
    - `create: "bean title"` — create a follow-up bean (inherits parent, gets verify from template)
    - `notify: "message"` — print/log a notification
  - Process on_close actions in cmd_close after verify passes and status is set to Closed

  ### 3. on_close in YAML serialization
  - Ensure on_close round-trips through YAML frontmatter
  - Skip serializing when empty

  ## Files
  - src/hooks.rs (add PostClose variant, update as_str)
  - src/bean.rs (add OnCloseAction enum, on_close field)
  - src/commands/close.rs (fire PostClose hook, process on_close actions)
  - tests/ (hook integration tests, on_close unit tests)

  ## Edge Cases
  - on_close `create` action should not block the close (fire-and-forget)
  - on_close `run` command failure should warn but not revert the close
  - PostClose hook failure should warn but not revert the close
  - Empty on_close vec should be skipped in serialization
closed_at: 2026-02-22T08:29:45.189748Z
close_reason: 'Auto-closed: all children completed'
verify: cargo test hooks
is_archived: true
tokens: 25760
tokens_updated: 2026-02-22T07:44:50.214741Z
