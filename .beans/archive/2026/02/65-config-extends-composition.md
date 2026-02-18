id: '65'
title: Config extends / composition
slug: config-extends-composition
status: closed
priority: 3
created_at: 2026-02-22T07:46:08.641125Z
updated_at: 2026-02-22T09:12:47.885578Z
description: "## Goal\nAllow beans config to inherit from parent configs via an extends field, inspired by Visor's composable extends system. Enables shared defaults across repos.\n\n## Motivation\nCurrently each project has a flat .beans/config.yaml with no inheritance. If you work across 10 repos, you set max_tokens, run command, max_attempts separately in each. Teams can't share conventions.\n\n## What to Build\n\n### 1. extends field in Config\n- `extends: Vec<String>` — list of paths to parent configs\n- Paths can be:\n  - Relative: `./team-standards.yaml`  \n  - Home-relative: `~/.beans/defaults.yaml`\n  - Absolute: `/etc/beans/org-defaults.yaml`\n- NOT remote URLs (security concern, keep it local-first)\n\n### 2. Config resolution\n- Load extends configs in order (first = lowest priority)\n- Merge: later values override earlier ones\n- Local .beans/config.yaml is highest priority (overrides everything)\n- Only merge known fields — don't blindly merge unknown YAML keys\n\n### 3. Which fields are inheritable\n- max_tokens ✓\n- max_loops ✓ (from bean 64)\n- run ✓\n- auto_close_parent ✓\n- project ✗ (always local)\n- next_id ✗ (always local)\n\n### 4. bn config show\n- Show resolved config with sources: 'max_tokens: 50000 (from ~/.beans/defaults.yaml)'\n- Show extends chain\n\n## Files\n- src/config.rs (extends field, load_with_extends, merge logic)\n- src/main.rs (use load_with_extends instead of Config::load)\n\n## Edge Cases\n- Circular extends: A extends B extends A → detect and error\n- Missing extends file: warn but continue (don't hard fail)\n- extends in an extended config: recursive resolution (with cycle detection)\n- File permissions: extended configs should be readable but not writable"
closed_at: 2026-02-22T09:12:47.885578Z
close_reason: 'Auto-closed: all children completed'
verify: cargo test config::tests
is_archived: true
tokens: 5185
tokens_updated: 2026-02-22T07:46:08.643580Z
