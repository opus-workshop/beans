id: '14'
title: Pi tools extension for beans ecosystem
slug: pi-tools-extension-for-beans-ecosystem
status: closed
priority: 2
created_at: 2026-02-03T05:36:19.891356Z
updated_at: 2026-02-03T05:44:55.366716Z
description: "## Goal\n\nCreate a pi extension that exposes beans, spro, and bw as native pi tools. This gives the LLM structured access to the task system instead of shelling out to bn commands.\n\n## Why\n\n- Better error handling (structured responses vs parsing stdout)\n- Type-safe parameters via TypeBox schemas\n- Custom rendering of tool calls/results in TUI\n- Session state integration (tool results become part of conversation)\n- Works with pi's context system\n\n## Structure\n\nSingle extension file: ~/.pi/agent/extensions/beans-tools.ts\n\nTool groups:\n1. **Core bn tools**: status, ready, quick, claim, close, verify\n2. **Context tools**: show, context, tree, list\n3. **Spro tools**: run, spawn, status, logs  \n4. **Watcher tools**: bw_status, bw_start, bw_stop\n\n## Acceptance\n\n- Extension loads without errors\n- All tools callable by LLM\n- Tools return structured JSON\n- Custom rendering for key tools (status, tree)"
closed_at: 2026-02-03T05:44:55.366716Z
close_reason: |-
  Created beans-tools.ts extension with complete tool coverage:

  **Core bn tools (6):**
  - bn_status, bn_quick, bn_create, bn_claim, bn_close, bn_verify

  **Context tools (3):**
  - bn_show, bn_context, bn_list

  **Spro tools (4):**
  - spro_run, spro_spawn, spro_status, spro_logs

  **Watcher tools (1):**
  - bn_watcher (with status/start/stop/once actions)

  **Decompose tools (3):**
  - decompose_assess, decompose_propose, decompose_create_child

  **Additional features:**
  - Status widget showing claimed/ready/blocked beans
  - Custom rendering for all tools (renderCall/renderResult)
  - /beans command for interactive bean selection
  - Ctrl+B shortcut for quick status check
  - Session event handlers to update widget on start/end
verify: test -f ~/.pi/agent/extensions/beans-tools.ts && pi --extensions 2>&1 | grep -q beans-tools
claimed_at: 2026-02-03T05:41:50.580605Z
is_archived: true
