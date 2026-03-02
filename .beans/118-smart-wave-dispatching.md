---
id: '118'
title: Smart wave dispatching
slug: smart-wave-dispatching
status: open
priority: 2
created_at: '2026-03-02T02:33:20.146517Z'
updated_at: '2026-03-02T02:33:20.146517Z'
verify: cargo test --lib -- commands::run 2>&1 | grep -E '(test result|FAILED)' | grep -v FAILED
tokens: 170
tokens_updated: '2026-03-02T02:33:20.149248Z'
---

Add three scheduling improvements to `bn run`:

1. **Critical-path prioritization** — Compute downstream weights so beans that block the most work get scheduled first
2. **File-conflict avoidance** — Don't run beans that touch the same files concurrently; defer conflicting beans until the file is free
3. **Smarter dry-run display** — Show critical path, file conflict groups, and effective parallelism per wave

Currently, the ready-queue sorts beans by `priority` field then ID, starts all ready beans simultaneously, and the dry-run just lists static waves. These changes make scheduling aware of the dependency graph structure and file contention.
