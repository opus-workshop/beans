id: '45'
title: Fix pi lock file for parallel agent spawning
slug: fix-pi-lock-file-for-parallel-agent-spawning
status: open
priority: 2
created_at: 2026-02-18T06:29:13.606480Z
updated_at: 2026-02-18T06:29:13.606480Z
description: |-
  ## Problem
  When deli spawns multiple pi agents in parallel (via `pi -p`), they fight over the same `proper-lockfile` lock in the project settings directory. Only one instance can acquire it; the others crash immediately with:

  ```
  Error: Lock file is already being held
      at proper-lockfile/lib/lockfile.js:68:47
  ```

  This makes parallel agent execution impossible — which is deli's entire purpose.

  ## Observed in
  `/Users/asher/.nvm/versions/node/v24.13.1/lib/node_modules/@mariozechner/pi-coding-agent/`

  The lock is acquired during startup for project settings and global settings.

  ## Root Cause
  pi uses `proper-lockfile` to lock settings files during read/write. When multiple pi instances start simultaneously in the same project directory, they all try to lock the same settings file. The lock is exclusive, so only one succeeds.

  ## Suggested Fix
  Options (pick one):
  1. **Per-session lock files** — include PID or session ID in the lock file path so parallel instances don't collide
  2. **Read-only settings for `-p` mode** — print mode agents don't need to write settings. Skip the lock entirely in non-interactive mode.
  3. **Retry with jitter** — add randomized backoff so parallel instances stagger their lock acquisition (fragile, not recommended)
  4. **Lock-free reads** — only lock when writing settings, use atomic reads without locking

  Option 2 is cleanest: `pi -p` is a fire-and-forget subagent that should never modify project settings.

  ## Files
  - node_modules/@mariozechner/pi-coding-agent/dist/core/ (settings loading)
  - node_modules/@mariozechner/pi-coding-agent/dist/main.js (startup sequence)

  ## Reproduction
  ```bash
  # In a project with .beans/
  for i in 1 2 3 4; do
    pi -p 'echo hello' &
  done
  wait
  # Most instances crash with lock file error
  ```
tokens: 456
tokens_updated: 2026-02-18T06:29:13.608527Z
