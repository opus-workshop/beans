---
id: '47'
title: Bean::to_file always writes pure YAML, losing markdown frontmatter format
slug: beantofile-always-writes-pure-yaml-losing-markdown
status: closed
priority: 2
created_at: 2026-02-18T06:54:10.660972Z
updated_at: 2026-02-18T08:46:43.795630Z
closed_at: 2026-02-18T08:46:43.795630Z
verify: cd /Users/asher/beans && cargo test -- --test-threads=1 bean::tests::file_round_trip bean::tests::test_file_round_trip_with_markdown 2>&1 | grep -q "ok"
claimed_at: 2026-02-18T08:36:59.938310Z
is_archived: true
tokens: 15442
tokens_updated: 2026-02-18T06:54:10.662583Z
---

**Problem:** `Bean::to_file()` in `src/bean.rs` always serializes to pure YAML via `serde_yaml::to_string()`, even when the file has a `.md` extension and was originally in markdown frontmatter format.

This means any operation that reads a `.md` bean and writes it back (claim, close, update, edit) silently converts it from:

```markdown
---
id: "1"
title: My Bean
---
# Description
Markdown body here
```

To pure YAML:
```yaml
id: '1'
title: My Bean
description: "# Description\nMarkdown body here"
```

The file still has a `.md` extension but no longer contains frontmatter. Subsequent reads still work (via the YAML fallback in `from_string`), but the format is degraded and the file is no longer human-readable as markdown.

**Affected commands:** All commands that modify and save beans — `claim`, `close`, `update`, `edit` (via `validate_and_save`)

**Fix options:**
1. Add a `to_file_md()` method that writes frontmatter format for `.md` files
2. Detect the original format during `from_file` and preserve it on `to_file`
3. Always write frontmatter format for `.md` files in `to_file`

**Files:**
- `src/bean.rs` (line 317, `to_file` method)
- `src/commands/edit.rs` (`validate_and_save` also re-serializes to YAML)
