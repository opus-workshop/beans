id: '5'
title: 'FEATURE: bn doctor should detect stale imports and format issues'
status: closed
priority: 3
created_at: 2026-02-02T10:00:00Z
updated_at: 2026-02-03T01:54:14.784513Z
description: |
  ## Problem
  `bn doctor` doesn't detect common issues from imports/migrations:
  - Mixed .yaml/.md formats
  - Duplicate IDs
  - Orphaned children (parent archived but children still reference it)
  - Stale index entries

  ## Expected
  `bn doctor` should report:
  ```
  ⚠ Found 211 .yaml files and 10 .md files (mixed formats)
  ⚠ Duplicate ID '193' in 2 files
  ⚠ Bean 193.1 references parent '193' which is archived
  ⚠ Index has 50 entries without source files

  Run `bn doctor --fix` to resolve
  ```

  ## Current bn doctor output
  Only checks for cycles and orphans in active beans.
closed_at: 2026-02-03T01:54:14.784513Z
close_reason: "Implemented enhanced bn doctor command with detection for:\n- Mixed .yaml/.md formats\n- Duplicate IDs (reports without failing build)\n- Orphaned dependencies  \n- Missing parents\n- Archived parents (children referencing archived parent)\n- Stale index entries\n- Cycles\n\nAdded --fix flag to automatically resolve fixable issues (rebuilds index).\nAll 10 doctor tests pass."
verify: cargo test commands::doctor -- --nocapture
attempts: 1
claimed_at: 2026-02-03T01:51:08.838664Z
is_archived: true
