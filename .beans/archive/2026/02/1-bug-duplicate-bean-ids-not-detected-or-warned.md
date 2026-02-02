id: '1'
title: 'BUG: Duplicate bean IDs not detected or warned'
status: closed
priority: 1
created_at: 2026-02-02T10:00:00Z
updated_at: 2026-02-03T01:10:46.712010Z
description: "## Problem\nWhen two bean files have the same `id:` field, `bn sync` and `bn list` \nsilently accept both. This causes confusing duplicate entries in the list.\n\n## Reproduction\n```bash\n# Create two files with same ID\necho \"id: '99'\\ntitle: Bean A\" > .beans/99-a.md\necho \"id: '99'\\ntitle: Bean B\" > .beans/99-b.md\nbn sync\nbn list  # Shows both, or unpredictable behavior\n```\n\n## Expected\n`bn sync` should error or warn when duplicate IDs are found.\n\n## Impact\n- Confusing list output\n- Parent/child relationships break\n- Queries return unexpected results\n"
closed_at: 2026-02-03T01:10:46.712010Z
close_reason: Added duplicate ID detection in Index::build(). When building the index, we now track ID-to-file mappings and return an error if the same ID appears in multiple files. Added 2 unit tests for the feature.
verify: cargo test index::tests::build_detects_duplicate
claimed_at: 2026-02-03T01:08:27.378067Z
is_archived: true
