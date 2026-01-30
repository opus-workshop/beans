---
id: 13
title: Write migration script to convert old YAML beans to .md
status: open
priority: 1
created_at: 2026-01-30T00:00:00Z
updated_at: 2026-01-30T00:00:00Z
parent: 11
type: task
---

# Write migration script to convert old YAML beans to .md

## What

Create a one-time script that:
1. Scans git history for all `*.yaml` bean files (currently deleted from working tree)
2. Reconstructs each bean from git
3. Extracts YAML metadata and block-scalar description
4. Writes new `.md` file with YAML frontmatter + markdown body
5. Preserves git history (created_at, updated_at from original)

## Implementation

Create `tools/migrate_beans.sh` (or Rust binary):

```bash
#!/bin/bash
# For each deleted bean in git history:
#   git show HEAD~N:.beans/1.yaml | convert_to_md > .beans/1-{slug}.md
```

**Algorithm:**
- Parse YAML with `yq` or manual parsing
- Extract all keys EXCEPT `description` → frontmatter YAML
- Extract `description` field (the block scalar) → markdown body
- Write frontmatter + body to `.md` file with naming `{id}-{slug}.md`

**Input:** Old YAML bean
```yaml
id: 1
title: Project scaffolding
description: |
  Some text
  ### Code
  ```rust
  pub struct Foo {}
  ```
acceptance: |
  - [ ] Done
```

**Output:** New `.md` bean (e.g., `1-project-scaffolding.md`)
```markdown
---
id: 1
title: Project scaffolding
acceptance: |
  - [ ] Done
---

Some text

### Code
```rust
pub struct Foo {}
```
```

## Files to create

- `tools/migrate_beans.sh` — Main migration script
- `tools/migrate_beans.rs` — (Optional) Type-safe Rust version using serde_yaml

## Acceptance

- [ ] Script runs without errors
- [ ] All beans from git history converted to `.md` format
- [ ] Metadata preserved (id, title, status, etc.)
- [ ] Descriptions with code blocks intact and readable
- [ ] Output validates against new schema
- [ ] Can be run idempotently (safe to re-run)
- [ ] Files named `{id}-{slug}.md`

## Run

```bash
./tools/migrate_beans.sh
ls .beans/*.md | wc -l  # Should match count of old beans
```
