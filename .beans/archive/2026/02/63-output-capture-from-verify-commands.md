id: '63'
title: Output capture from verify commands
slug: output-capture-from-verify-commands
status: closed
priority: 2
created_at: 2026-02-22T07:45:39.454214Z
updated_at: 2026-02-22T08:53:38.095097Z
description: "## Goal\nAllow beans to capture structured output from verify commands so downstream beans can consume data, not just pass/fail status. Inspired by Visor's schema-validated step outputs.\n\n## Motivation\nCurrently produces/requires are string labels for dependency ordering. But there's no way to pass DATA between beans. If bean A runs a security scan and finds 5 issues, bean B (fix issues) has no structured way to receive that list. The failure output is buried in notes as markdown text.\n\n## What to Build\n\n### 1. outputs field on Bean\n- `pub outputs: Option<serde_json::Value>` on Bean struct  \n- Populated when verify command succeeds and produces JSON to stdout\n- Stored in YAML frontmatter (or separate .outputs.json file if large)\n\n### 2. Output capture in close\n- When verify command runs, capture stdout separately from stderr\n- If stdout is valid JSON, store it in bean.outputs\n- If not JSON, store as `{ \"text\": \"...\" }` wrapper\n- Add `--capture-output` flag (or make it default when outputs field exists)\n\n### 3. Output reference in descriptions\n- Convention: `{{outputs.60.field}}` in bean descriptions\n- bn context could expand these references when building agent prompts\n- Or: just document the convention and let agents use `bn show <dep-id>` to read outputs\n\n### 4. bn show integration  \n- Display outputs section in `bn show` when present\n- `bn show <id> --outputs` for just the JSON\n\n## Files\n- src/bean.rs (outputs field, serialization)\n- src/commands/close.rs (capture stdout, parse JSON, store)\n- src/commands/show.rs (display outputs)\n\n## Edge Cases\n- Large output: cap at some size (e.g., 64KB), truncate with warning\n- Binary output: skip capture, warn\n- Multiple verify commands (chained with &&): only capture last command's stdout\n- Output should not bloat .md files — consider .beans/<id>.outputs.json for large payloads"
closed_at: 2026-02-22T08:53:38.095097Z
close_reason: 'Auto-closed: all children completed'
verify: cargo test output
is_archived: true
tokens: 22432
tokens_updated: 2026-02-22T07:45:39.456309Z
