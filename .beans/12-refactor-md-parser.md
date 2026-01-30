---
id: 12
title: Update Bean parser to read .md + YAML frontmatter
status: open
priority: 1
created_at: 2026-01-30T00:00:00Z
updated_at: 2026-01-30T00:00:00Z
parent: 11
type: task
---

# Update Bean parser to read .md + YAML frontmatter

## What

Modify `src/bean.rs` to parse Markdown files with YAML frontmatter instead of pure YAML.

**File format:**
```markdown
---
id: 1
title: Foo
status: open
priority: 0
created_at: 2026-01-26T15:00:00Z
---

# Markdown body here

Code and prose mixed naturally.
```

## Implementation

1. Update `Bean::from_file()` to:
   - Read file content as string
   - Find first `---\n` and second `---\n` boundaries
   - Parse content between first and second as YAML → metadata struct
   - Keep content after second `---` as `description: String`

2. Preserve existing metadata fields:
   - id, title, status, priority, created_at, updated_at
   - Add optional: parent, dependencies, labels

3. Handle edge cases:
   - Files starting with `---` (already markdown frontmatter)
   - `---` in the body (after the second delimiter)
   - Empty body (frontmatter-only bean)

## Code location

- `src/bean.rs:32` — `impl Bean { fn from_file() }`

## Acceptance

- [ ] Parses `.md` files with YAML frontmatter correctly
- [ ] Preserves all metadata fields
- [ ] Body (after `---`) stored as `description`
- [ ] Handles `---` in markdown body without breaking
- [ ] `cargo test` passes for bean parsing
- [ ] Works with both old beans (reconstructed from git) and new ones

## Testing

```rust
#[test]
fn test_parse_md_frontmatter() {
    let content = r#"---
id: 12
title: Test Bean
status: open
---

# Description

Test markdown body.
"#;
    let bean = Bean::from_string(content).unwrap();
    assert_eq!(bean.id, 12);
    assert!(bean.description.contains("# Description"));
}
```
