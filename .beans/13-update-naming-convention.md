---
id: 13
title: Update bn to support {id}-{slug}.md naming convention
status: open
priority: 1
created_at: 2026-01-30T00:00:00Z
updated_at: 2026-01-30T00:00:00Z
type: epic
---

# Update bn to support {id}-{slug}.md naming convention

## Why

Beans now use `{id}-{slug}.md` filenames (e.g., `11-refactor-md-frontmatter.md`, `11.1-refactor-md-parser.md`) but the `bn` binary assumes flat filenames (`1.yaml`, `2.yaml`). The CLI needs to:

- Locate beans by ID even when filename contains slug
- Generate proper slugs from titles on create
- Update file operations (move, delete, copy) to handle new format
- Support both hierarchical IDs (11.1) and flat IDs (11)

## What

Update `bn` core operations to work with `{id}-{slug}.md`:

1. **Lookup by ID:** `bn show 11.1` finds `.beans/11.1-*.md` (glob by id prefix)
2. **Create with slug:** `bn create --title="Foo Bar"` creates `N-foo-bar.md` (auto-slug)
3. **File operations:** Move/rename/delete work with full filename
4. **Index:** Track both id and slug for fast lookup

## Implementation scope

- `src/lib.rs` — Update file discovery logic (glob for `{id}-*` pattern)
- `src/bean.rs` — Ensure slug is extracted/stored in metadata
- `src/commands/create.rs` — Generate slug from title
- `src/util.rs` — Add slug generation utility (title → kebab-case)
- All command handlers — Update file path handling
- Tests — Verify lookups work with hierarchical IDs

## Acceptance

- [ ] `bn show 11` finds `11-refactor-md-frontmatter.md`
- [ ] `bn show 11.1` finds `11.1-refactor-md-parser.md`
- [ ] `bn create --title="My Task"` creates `N-my-task.md`
- [ ] `bn list` works correctly (no duplicates, correct count)
- [ ] File operations (rename/move/delete) preserve full filename
- [ ] Index rebuilt correctly with new naming
- [ ] Slug generation handles special characters, spacing, case
- [ ] All existing commands work without modification
- [ ] `cargo test` passes

## Files to modify

- `src/lib.rs` — Bean discovery/lookup
- `src/bean.rs` — Slug handling
- `src/commands/create.rs` — Slug generation on create
- `src/util.rs` — Slug utility function
- `src/commands/*.rs` — All file path operations

## Notes

Slug generation rules:
- Title: "Implement `bn show` to render Markdown" → slug: "implement-bn-show-to-render-markdown"
- Lowercase, alphanumeric + hyphens
- Truncate to 50 chars if needed
- No consecutive hyphens
