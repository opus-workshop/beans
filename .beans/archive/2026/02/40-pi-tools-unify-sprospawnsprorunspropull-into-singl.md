id: '40'
title: 'pi tools: unify spro_spawn/spro_run/spro_pull into single spro tool'
slug: pi-tools-unify-sprospawnsprorunspropull-into-singl
status: closed
priority: 2
created_at: 2026-02-05T10:30:25.263794Z
updated_at: 2026-02-05T10:40:20.458317Z
description: |-
  ## Contract

  After spro CLI is unified, update pi tools to match:

  **Current tools:**
  - `spro_spawn(beanId, ...)` - single bean
  - `spro_run(parentId, ...)` - children, blocking
  - `spro_pull(parentId, ...)` - children, background

  **New unified tool:**
  ```typescript
  spro(id, {
    wait?: boolean,      // default false (background)
    parallel?: number,   // default 4
    dryRun?: boolean,
    keepGoing?: boolean,
    timeout?: number,
    idleTimeout?: number
  })
  ```

  ## Examples

  ```typescript
  spro("32")                    // children in background
  spro("32.1")                  // single bean in background
  spro("32", { wait: true })    // blocking
  spro("32", { parallel: 8 })   // more parallelism
  spro("32", { dryRun: true })  // preview
  ```

  ## Deprecation

  Keep old tools as aliases with deprecation warning:
  - `spro_spawn(id)` → `spro(id)`
  - `spro_run(id)` → `spro(id, { wait: true })`
  - `spro_pull(id)` → `spro(id)`

  ## Dependencies

  Requires bean 39 (spro CLI unification) to be completed first.
closed_at: 2026-02-05T10:40:20.458317Z
close_reason: 'Updated pi extension: unified spro tool with background default, deprecated aliases'
verify: echo "Depends on bean 39 - verify manually after spro CLI updated"
is_archived: true
requires:
- spro_unified_cli
tokens: 267
tokens_updated: 2026-02-05T10:30:25.265460Z
