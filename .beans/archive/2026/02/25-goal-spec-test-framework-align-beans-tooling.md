id: '25'
title: 'Goal-Spec-Test framework: align beans tooling'
slug: goal-spec-test-framework-align-beans-tooling
status: closed
priority: 2
created_at: 2026-02-05T07:38:17.488398Z
updated_at: 2026-02-05T07:40:22.127015Z
description: "## Goal\n\nAlign beans tooling with the Goal → Spec → Test development framework:\n- **Goal** (parent bean): WHY - the outcome we want\n- **Spec** (child bean): WHAT - concrete contract, inputs/outputs, edge cases  \n- **Test** (verify command): Proves the spec is met (TDD)\n\n## Context\n\nThe decompose skill has been updated with this philosophy. Now we need to:\n1. Update the beans skill to reflect this framework\n2. Update auto-decompose skill to use spec-driven principles\n3. Improve bn claim error message for too-large beans\n4. Add token size feedback to bn create (especially for --run flag)\n\n## Files\n- /Users/asher/.pi/agent/skills/beans/SKILL.md\n- /Users/asher/.pi/agent/skills/auto-decompose/SKILL.md\n- src/commands/claim.rs\n- src/commands/create.rs\n\n## Key insight\n\nWhen using `--run` to spawn an agent, give feedback about whether the bean is:\n- Right-sized (good for implementation)\n- Too large (needs decomposition first)\n- A goal vs a spec"
closed_at: 2026-02-05T07:40:22.127015Z
close_reason: 'Auto-closed: all children completed'
verify: cargo test && grep -q "Goal" /Users/asher/.pi/agent/skills/beans/SKILL.md
is_archived: true
tokens: 16131
tokens_updated: 2026-02-05T07:38:17.489978Z
