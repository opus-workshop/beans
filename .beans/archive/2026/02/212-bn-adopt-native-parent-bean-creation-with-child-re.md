id: '212'
title: bn adopt - Native parent bean creation with child renumbering
slug: bn-adopt-native-parent-bean-creation-with-child-re
status: closed
priority: 1
created_at: 2026-02-02T05:22:34.524259Z
updated_at: 2026-02-02T06:02:47.126888Z
description: |-
  ## Problem
  Creating parent beans and adopting existing beans as children requires manual YAML editing. This should be native to bn.

  ## Proposed Commands

  ### Adopt existing beans under a parent
  ```bash
  bn adopt 211 203 204 205 206 207 208 209 210
  # Result:
  #   211 becomes parent
  #   203 -> 211.1 (renumbered)
  #   204 -> 211.2
  #   etc.
  ```

  ### Create parent and adopt in one step
  ```bash
  bn create "Parent Title" --adopt 203,204,205
  # Creates new parent, adopts and renumbers children
  ```

  ## Naming Convention
  Children of parent X should be numbered X.1, X.2, X.3, etc.
  Files: X.1-slug.md, X.2-slug.md

  ## Current Workaround (manual)
  ```bash
  sed -i '' "s/^id: 'N'/id: 'N'\nparent: 'P'/" .beans/N-*.md
  bn sync
  ```

  ## Implementation
  - Modify bn CLI (Rust)
  - Add adopt subcommand
  - Handle ID renumbering (N -> P.X)
  - Update all references (deps pointing to old IDs)
  - Rename files to match new IDs
  - Rebuild index

  ## Files
  - src/bin/bn.rs or wherever bn CLI lives
closed_at: 2026-02-02T06:02:47.126888Z
verify: bn adopt --help 2>&1 | grep -qi adopt
is_archived: true
