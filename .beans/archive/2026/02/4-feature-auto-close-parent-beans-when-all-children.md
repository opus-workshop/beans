id: '4'
title: 'FEATURE: Auto-close parent beans when all children archived/closed'
status: closed
priority: 3
created_at: 2026-02-02T10:00:00Z
updated_at: 2026-02-03T01:54:16.041432Z
description: |
  ## Problem
  Parent beans remain open even after all children are closed and archived.
  User must manually close parent, but verify commands often fail because
  they can't find archived children (see bug #2).

  ## Current Behavior
  ```bash
  # Parent 211 has 8 children, all closed & archived
  bn list  # Still shows [ ] 211 as open
  bn close 211  # Verify fails - can't find archived children
  ```

  ## Expected
  Option 1: Auto-close parent when last child closes
  Option 2: `bn close --force` to skip verify
  Option 3: Parent verify should check archive

  ## Use Case
  Wave/parallel execution creates parent beans with many children.
  When wave completes, parent should auto-close or be easy to close.
closed_at: 2026-02-03T01:54:16.041432Z
close_reason: Implemented `bn close --force` flag to skip verify. Auto-close parent and archive checking were already implemented.
verify: cargo test --release close -- --test-threads=1 2>&1 | grep -q "42 passed"
claimed_at: 2026-02-03T01:51:08.841711Z
is_archived: true
