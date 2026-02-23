---
id: '81'
title: Absorb deli into bn and simplify agent integration
slug: absorb-deli-into-bn-and-simplify-agent-integration
status: closed
priority: 2
created_at: 2026-02-23T23:49:35.904758Z
updated_at: 2026-02-24T01:08:25.576474Z
closed_at: 2026-02-24T01:08:25.576474Z
close_reason: 'Auto-closed: all children completed'
is_archived: true
tokens: 883
tokens_updated: 2026-02-23T23:49:35.905932Z
---

## Goal

Merge deli's functionality into bn and simplify the agent ecosystem so that:
1. `bn run` does everything deli does (parallel spawning, JSON streaming, timeouts, progress tracking)
2. The deli binary is retired
3. The pi extension talks to `bn run --json-stream` instead of `deli --json-stream`
4. The auto skill's decomposition wisdom is baked into `bn plan`'s template
5. The beans skill is simplified to a minimal reference

## Context

Currently the agent ecosystem has too many moving parts:
- `bn` (Rust) — task tracking, basic agent dispatch
- `deli` (separate Rust binary, ~3.5k lines) — parallel spawning, JSON streaming, progress, timeouts, worktree isolation
- `auto` skill (SKILL.md) — decomposition instructions for agents
- `beans` skill (SKILL.md) — how-to-use-beans instructions
- `deli` extension (TypeScript) — TUI for deli progress in pi
- `beans` extension (TypeScript) — status widget in pi

The goal is to collapse this to:
- `bn` (Rust) — does everything: track, dispatch, stream, monitor
- `beans` extension (TypeScript) — merged TUI (status + progress), talks to bn
- Minimal beans skill or AGENTS.md entry

## What deli has that bn needs

### JSON streaming events (`--json-stream`)
Stream types from deli/src/stream.rs:
- RunStart, RoundStart, BeanStart, BeanThinking, BeanTool, BeanTokens, BeanDone, RoundEnd, RunEnd, DryRun, Error

### Pi JSON output parsing (deli/src/agent.rs + json_output.rs)
Reads pi's `--mode json` stdout to extract:
- Thinking deltas, text deltas, tool calls (start/end with arguments), token usage, cost
- Parses turn_end events for cumulative token tracking

### Timeout system (deli/src/timeout.rs)
- Total timeout per bean (default 30min)
- Idle timeout — kill if no stdout for N minutes (default 5min)
- Process monitoring in a separate thread

### Worktree isolation (deli/src/worktree.rs)
- Git worktree create/merge/cleanup per agent
- Merge conflict detection
- Already partially implemented in bn (bean 78)

### Wave execution (deli/src/wave.rs)
- Parallel execution with thread pool
- Wave-based: round 1 (no deps), round 2 (deps on round 1), etc.
- bn already has compute_waves in orchestrator.rs — needs the execution part

### Status tracking (deli/src/status.rs)
- JSONL files tracking agent progress
- Per-parent status files

### Rich TUI (deli/src/display.rs, live.rs, ui.rs)
- Single-bean spinner display
- Multi-bean live progress display
- Summary table after completion

## Architecture for absorption

### Phase 1: Add --json-stream to bn run
- Add StreamEvent types to bn (from deli/src/stream.rs)
- Add pi output parsing (from deli/src/agent.rs)
- Add timeout monitoring (from deli/src/timeout.rs)
- bn run spawns pi directly (not via template) with --mode json --print --no-session
- Monitor stdout, emit StreamEvent JSON lines

### Phase 2: Update pi extension
- Rename deli extension to beans (merge with existing beans extension)
- Change spawn calls from `deli` to `bn run --json-stream`
- Keep all TUI components (DeliProgressComponent, etc.)

### Phase 3: Bake auto into bn plan
- bn plan --auto spawns an agent with decomposition prompt baked in
- The prompt includes: sizing rules, split strategies, context embedding rules, retry logic
- This replaces the auto skill entirely

### Phase 4: Simplify skills
- Reduce beans skill to a minimal reference card
- Delete auto skill
- Delete orchestrate skill (just created, superseded by this work)
- Delete decompose skill
