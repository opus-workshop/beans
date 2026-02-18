id: '52'
title: cmd_quick requires acceptance or verify, but cmd_create --claim allows neither — inconsistent UX
slug: cmdquick-requires-acceptance-or-verify-but-cmdcrea
status: open
priority: 3
created_at: 2026-02-18T06:54:24.151484Z
updated_at: 2026-02-18T06:54:24.151484Z
description: "**Problem:** There's an inconsistency in validation between `bn quick` and `bn create --claim`:\n\n- `bn quick \"task\"` → **Error**: \"Bean must have validation criteria\"\n- `bn create \"task\" --claim` → **Success**: Creates and claims with no verification criteria\n\nBoth commands create beans meant for immediate work, but `quick` is stricter. This creates confusing UX where the \"shortcut\" command is harder to use than the full command.\n\n**Options:**\n1. Add the same validation to `create --claim` \n2. Remove the validation from `quick` (allow quick beans without verify/acceptance)\n3. Document the difference prominently\n4. Add a `--goal` flag to `quick` to explicitly opt out\n\n**Files:**\n- `src/commands/quick.rs` (validation check near top of `cmd_quick`)\n- `src/commands/create.rs` (no equivalent validation)"
acceptance: Decision documented in code comments about why quick and create have different validation requirements
tokens: 13465
tokens_updated: 2026-02-18T06:54:24.154176Z
