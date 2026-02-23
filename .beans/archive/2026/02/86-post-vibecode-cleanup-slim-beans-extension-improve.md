---
id: '86'
title: 'Post-vibecode cleanup: slim beans extension, improve bn run'
slug: post-vibecode-cleanup-slim-beans-extension-improve
status: closed
priority: 2
created_at: 2026-02-25T07:05:09.972178Z
updated_at: 2026-02-25T07:47:28.843018Z
closed_at: 2026-02-25T07:47:28.843018Z
close_reason: 'Auto-closed: all children completed'
is_archived: true
tokens: 212
tokens_updated: 2026-02-25T07:05:09.976456Z
---

## Context

We vibecoded a pi-native beans extension (~/.pi/agent/extensions/beans/) that replaced `bn run` with TypeScript-based orchestration. Retroactive planning identified:

1. **IPC system (bean_ask/bean_respond) has no real use case** — built speculatively, should be removed
2. **Ready-queue scheduling is good but belongs in bn (Rust)** — keeps bn tool-agnostic
3. **Extension should be a thin pi-specific layer** — spawning, prompts, TUI — not a bn reimplementation
4. **Nothing is tested**

## Plan

Phase 1: Remove IPC from extension (bean_ask, bean_respond, ipc.ts)
Phase 2: Improve bn run in Rust with ready-queue dispatch (84.14 prerequisite chain)
Phase 3: Add tests for extension scheduler + spawner
Phase 4: Update extension SKILL.md and README to reflect changes
