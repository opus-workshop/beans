id: '12'
title: Implement bean archive system
status: closed
priority: 2
created_at: 2026-01-30T00:00:00Z
updated_at: 2026-01-31T17:43:31.140093Z
description: |
  # Implement bean archive system

  ## Why

  As beans accumulate, `.beans/` gets cluttered. Closed beans should be archivable:
  - Move old/completed work to `.beans/archive/`
  - Keep working directory clean and fast
  - Preserve history (git still has it)
  - Can unarchive if needed

  ## What

  1. New command: `bn archive <bean-id>`
     - Moves bean file to `.beans/archive/<year>/<month>/<bean-id>-<slug>.md`
     - Updates index
     - Commits to git with message `[archive] <id>`

  2. New command: `bn unarchive <bean-id>`
     - Moves bean back to `.beans/<bean-id>-<slug>.md`
     - Updates index
     - Commits to git

  3. `bn list --archived` — Show archived beans
  4. `bn list` — Exclude archived by default

  ## Structure

  ```
  .beans/
    ├── index.yaml
    ├── config.yaml
    ├── 1-project-scaffolding.md (active)
    ├── 2-bean-model.md (active)
    └── archive/
        ├── 2026/
        │   ├── 01/
        │   │   ├── 100-old-feature.md (closed 2026-01-15)
        │   │   └── 101-another-task.md
        │   └── 02/
        │       └── 102-completed-work.md
        └── 2025/
            └── 12/
                └── 50-legacy-bean.md
  ```

  ## Files to create/modify

  - `src/commands/archive.rs` — New `archive` subcommand
  - `src/commands/unarchive.rs` — New `unarchive` subcommand
  - `src/bean.rs` — Add `is_archived: bool` field
  - `src/lib.rs` — Update index to track archived beans
  - `.beans/index.yaml` — Extend to list archived paths

  ## Acceptance

  - [ ] `bn archive <id>` moves file to `.beans/archive/<year>/<month>/<id>-<slug>.md`
  - [ ] Index updated to mark bean as archived
  - [ ] `bn list` excludes archived by default
  - [ ] `bn list --archived` shows archived beans
  - [ ] `bn unarchive <id>` restores to `.beans/<id>-<slug>.md`
  - [ ] Both commands commit to git
  - [ ] Archives by date (year/month subdirs)
  - [ ] `bn show <archived-id>` still works

  ## Notes

  Archive by date so archives naturally separate old from recent, maintaining chronological order.
closed_at: 2026-01-31T17:43:31.140093Z
close_reason: archive system complete
is_archived: true
