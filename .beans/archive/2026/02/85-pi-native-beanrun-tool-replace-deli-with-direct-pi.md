---
id: '85'
title: 'Pi-native bean_run tool: replace deli_* with direct pi subprocess spawning'
slug: pi-native-beanrun-tool-replace-deli-with-direct-pi
status: closed
priority: 2
created_at: 2026-02-24T08:13:15.082470Z
updated_at: 2026-02-24T08:30:02.216015Z
closed_at: 2026-02-24T08:30:02.216015Z
verify: grep -q 'bean_run' ~/.pi/agent/extensions/beans/index.ts && grep -q 'bean_status' ~/.pi/agent/extensions/beans/index.ts && grep -q 'sendMessage' ~/.pi/agent/extensions/beans/index.ts
fail_first: true
is_archived: true
tokens: 3871
tokens_updated: 2026-02-24T08:13:15.088388Z
history:
- attempt: 1
  started_at: 2026-02-24T08:30:02.216918Z
  finished_at: 2026-02-24T08:30:02.237139Z
  duration_secs: 0.02
  result: pass
  exit_code: 0
---

# Pi-Native bean_run Tool

Replace the three deli_* tools (deli_run, deli_spawn, deli_pull) with a single `bean_run` tool that spawns pi subprocesses directly instead of shelling out to `bn run`.

## Design Doc
See DESIGN-bean-run-native.md in the repo root for full design.

## Architecture

**Current (eliminate):**
```
pi → deli_run → bn run --json-stream → pi --mode json (per bean)
```

**New:**
```
pi → bean_run → reads .beans/ directly → pi --mode json (per bean)
     ↓ state mutations via bn claim/close/verify/update
```

## Key Behaviors

### Background with notification (default)
- `bean_run` spawns agents and **returns immediately**
- Agents run in background, progress shown in TUI widget
- When agents complete/fail, extension calls `pi.sendMessage({ deliverAs: "followUp" })` to notify the orchestrator
- Orchestrator sees completions automatically, can react without polling
- `bean_status` tool still available for full overview

### Ready-queue (not strict waves)
- Instead of wave N+1 waiting for ALL of wave N, use a ready-queue
- When any bean finishes, immediately check if new beans are unblocked
- Bean C (depends on A only) starts as soon as A finishes, even if sibling B is still running

### Richer prompts
- Inject .beans/RULES.md into system prompt if it exists
- Bean markdown body as user message
- Instructions for closing: "run bn close {id} when done"
- Per-bean model override possible

## Tool Interface

```typescript
bean_run({
  target: "84",              // Bean ID - run children or single bean
  parallel: 4,               // Max concurrent agents (default: 4)
  dryRun: false,             // Preview execution plan
  keepGoing: false,          // Continue past failures
  timeout: 30,               // Minutes per agent
  idleTimeout: 5,            // Minutes of no output -> kill
  model: "claude-sonnet-4-5", // Override model for spawned agents
  instructions: "...",       // Prepend to each agent's task
})
```

**Mode inference:**
- Target has open children -> run children (multi-agent orchestration)
- Target is leaf with verify -> run single agent
- Target has no verify -> error

## Implementation Files

All in `~/.pi/agent/extensions/beans/`:

1. **parser.ts** - Read index.yaml and bean .md files (YAML frontmatter + body)
2. **scheduler.ts** - Ready-queue computation from produces/requires/dependencies
3. **spawner.ts** - Spawn pi subprocess, parse JSON events, manage lifecycle
4. **prompt.ts** - Build agent prompts from bean content + rules
5. **progress.ts** - TUI progress widget (adapt existing BeansProgressComponent)
6. **index.ts** - Register bean_run + bean_status tools, wire everything together

## State Mutations (still via bn CLI)

- `bn claim <id> --by pi-agent` - before spawning
- `bn close <id>` - agent runs this on success (verify gate)
- `bn update <id> --note "..."` - on failure
- `bn claim <id> --release` - release failed claims

## Key Dependencies

- `yaml` package (available via pi's node_modules)
- `node:child_process` spawn (for pi subprocesses)
- `@mariozechner/pi-coding-agent` types (ExtensionAPI, etc.)
- `@mariozechner/pi-tui` (Text, Container, truncateToWidth)
- `@sinclair/typebox` (tool parameter schemas)

## Remove

- `deli_run` tool
- `deli_spawn` tool
- `deli_pull` tool
- `deli_status` tool (replace with `bean_status`)
- `deli_logs` tool (replace with `bean_logs`)

## Keep

- `/beans` command (claim selector)
- `/beans:init` command
- `ctrl+b` shortcut (status)
- Status widget (aboveEditor)
- BeansProgressComponent (adapt for direct subprocess events)
