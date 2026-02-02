id: '11'
title: Migrate beans from YAML to Markdown + YAML Frontmatter
status: closed
priority: 1
created_at: 2026-01-30T00:00:00Z
updated_at: 2026-02-02T06:09:10.312873Z
description: |
  # Migrate beans from YAML to Markdown + YAML Frontmatter

  ## Why

  Current YAML-only format has friction:
  - Code snippets in block scalars break easily with indentation
  - Not human-readable when viewing raw files
  - GitHub doesn't render beautifully
  - LLMs waste tokens parsing YAML indentation
  - Can't use Obsidian vault features (graph view, wiki links)

  Markdown + YAML frontmatter (Jekyll/Hugo standard) solves all of this:
  - Code fences work naturally without escaping
  - GitHub renders as document with metadata table
  - Obsidian compatibility (free wiki)
  - Cleaner git diffs
  - Native LLM format

  ## What

  Convert all bean files from:
  ```yaml
  id: 1
  title: Foo
  description: |
    Some text
    ## Code
    ```rust
    ...
  ```

  To:
  ```markdown
  ---
  id: 1
  title: Foo
  ---

  # Description

  Some text

  ## Code
  ```rust
  ...
  ```

  ## Acceptance

  - [ ] Parser in `src/bean.rs` reads `.md` files with YAML frontmatter
  - [ ] Migration script converts old YAML beans to new `.md` format
  - [ ] CLI `show` command renders markdown body with `termimad` or similar
  - [ ] All tests pass with new format
  - [ ] Old `.beans/*.yaml` files migrated to `.beans/*.md`
  - [ ] Git history preserved in bean metadata (created_at, updated_at)

  ## Files to modify

  - `src/bean.rs` — Add MD parser (split on `---`, parse frontmatter as YAML, keep body as string)
  - `src/commands/show.rs` — Render markdown body nicely
  - New: `tools/migrate_beans.rs` or shell script — Convert old format to new

  ## Dependencies

  None (self-contained refactor)
closed_at: 2026-02-02T06:09:10.312873Z
is_archived: true
