---
id: '84'
title: 'Codebase improvements: robustness, DX, and maintainability'
slug: codebase-improvements-robustness-dx-and-maintainab
status: open
priority: 2
created_at: 2026-02-24T06:48:10.784339Z
updated_at: 2026-02-24T06:48:10.784339Z
tokens: 3455
tokens_updated: 2026-02-24T06:48:10.785888Z
---

## Overview

Top improvements identified during codebase recon (2026-02-23). Grouped into:

1. **Robustness** — file locking, atomic writes, signal handling
2. **DX** — CI pipeline, shell completions, task runner, serde_yaml migration
3. **Maintainability** — split god files, output abstraction, main.rs dispatch cleanup
4. **Docs** — CONTRIBUTING.md, CHANGELOG.md

See ARCHITECTURE.md Health & Risks section for full context.

## Priority Order

1. File locking on index.yaml (bug — parallel agents will corrupt)
2. Atomic writes (crash safety)
3. CI pipeline
4. Split close.rs
5. Shell completions
6. Everything else
