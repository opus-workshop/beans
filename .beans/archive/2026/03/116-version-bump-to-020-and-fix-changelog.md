---
id: '116'
title: Version bump to 0.2.0 and fix CHANGELOG
slug: version-bump-to-020-and-fix-changelog
status: closed
priority: 2
created_at: '2026-03-02T02:27:57.369298Z'
updated_at: '2026-03-02T02:29:06.953029Z'
labels:
- chore
- publish-prep
closed_at: '2026-03-02T02:29:06.953029Z'
verify: cd /Users/asher/beans && grep -q '^version = "0.2.0"' Cargo.toml && grep -q '\[0.2.0\]' CHANGELOG.md && ! grep -q '0.4.0' Cargo.toml
fail_first: true
checkpoint: '655ee759245f4c916a5cb00eff3a627366a732b7'
claimed_by: pi-agent
claimed_at: '2026-03-02T02:28:05.075837Z'
is_archived: true
tokens: 2145
tokens_updated: '2026-03-02T02:27:57.371011Z'
history:
- attempt: 1
  started_at: '2026-03-02T02:29:06.956352Z'
  finished_at: '2026-03-02T02:29:07.011360Z'
  duration_secs: 0.055
  result: pass
  exit_code: 0
attempt_log:
- num: 1
  outcome: success
  agent: pi-agent
  started_at: '2026-03-02T02:28:05.075837Z'
  finished_at: '2026-03-02T02:29:06.953029Z'
---

## Task
Change the version from 0.4.0 to 0.2.0 in Cargo.toml and update CHANGELOG.md to match.

## Steps

### 1. Cargo.toml
Change `version = "0.4.0"` to `version = "0.2.0"`.

### 2. CHANGELOG.md
- Merge the current `[Unreleased]` items into the `[0.2.0]` section (since they're shipping now)
- Replace the current `[0.4.0]` header with `[0.2.0]`
- Combine the current `[0.2.0]` section content INTO the new `[0.2.0]` section (merge both releases into one since we're rebasing the version history)
- Also fold in the `[0.1.0]` content — this is the first public release, so everything goes under `[0.2.0]`
- Keep the `[Unreleased]` header but make it empty
- Fix the comparison links at the bottom:
  - `[Unreleased]` should compare `v0.2.0...HEAD`
  - `[0.2.0]` should link to the release tag
- Remove old version section headers and links for 0.4.0 and 0.1.0

### 3. Cargo.lock
Run `cargo check` to update Cargo.lock with the new version.

## Don't
- Don't change anything else in Cargo.toml besides the version
- Don't remove any CHANGELOG entries — merge them all under 0.2.0
- Don't rewrite entries — keep the existing text, just reorganize under the new version
