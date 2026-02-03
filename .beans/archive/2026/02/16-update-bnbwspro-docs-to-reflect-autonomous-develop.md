id: '16'
title: Update bn/bw/spro docs to reflect autonomous development workflow
slug: update-bnbwspro-docs-to-reflect-autonomous-develop
status: closed
priority: 2
created_at: 2026-02-03T06:58:36.764634Z
updated_at: 2026-02-03T07:00:33.920399Z
description: |-
  ## Context

  We just demonstrated the full autonomous development flywheel:
  1. Human creates bean family with produces/requires dependencies
  2. bw daemon notices unclaimed beans
  3. bw spawns pi agents to implement (or decompose if too large)
  4. Agents verify and close beans
  5. Dependencies unblock next wave
  6. Repeat until done

  This resulted in a 1579-line pi extension implemented entirely by bw-spawned agents.

  ## Tasks

  Update documentation to explain:
  - The flywheel concept (beans → bw → agents → verify → close → unblock)
  - How to create bean families with proper dependencies
  - bw configuration (allowed.toml: auto/ask/never per project)
  - The pi tools extension that enables native bn_*/spro_*/bw_* tools
  - Example workflow from this session

  ## Files
  - README.md (main beans readme)
  - docs/AUTONOMOUS.md (new - detailed autonomous workflow)
  - docs/BW.md (bean watcher docs)
closed_at: 2026-02-03T07:00:33.920399Z
close_reason: Added autonomous development section to README and created pi extension docs
verify: grep -q 'autonomous' docs/README.md || grep -q 'bw' README.md
claimed_at: 2026-02-03T06:58:36.764633Z
is_archived: true
