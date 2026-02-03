id: '3'
title: 'BUG: Mixed .yaml/.md bean formats cause confusion'
status: closed
priority: 3
created_at: 2026-02-02T10:00:00Z
updated_at: 2026-02-03T01:42:15.003962Z
description: "## Problem\n`bn sync` reads both `.yaml` and `.md` files from `.beans/` directory.\nWhen migrating from beads (which used .yaml), leftover .yaml files \nget indexed alongside new .md files, inflating bean count.\n\n## Reproduction\n```bash\n# After beads migration, .beans/ has:\nls .beans/*.yaml | wc -l  # 211 old yaml files\nls .beans/*.md | wc -l    # 10 new md files\nbn sync                    # \"219 beans indexed\"\nbn list                    # Shows 200+ beans, mostly stale\n```\n\n## Expected\nEither:\n1. Warn when both formats present\n2. Provide migration command to convert/archive .yaml files\n3. Add config option to specify preferred format\n4. `bn doctor` should flag this\n\n## Workaround\nManually delete or move .yaml files:\n```bash\nrm .beans/*.yaml\nbn sync\n```\n"
closed_at: 2026-02-03T01:42:15.003962Z
close_reason: 'Implemented mixed format detection: bn sync and bn doctor now warn when both .yaml and .md bean files are present, with instructions to migrate legacy files'
verify: cargo test --lib -- doctor_detects_mixed_formats && cargo test --lib -- count_bean_formats
attempts: 2
claimed_at: 2026-02-03T01:38:42.477684Z
is_archived: true
