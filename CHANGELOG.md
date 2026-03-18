# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.0] - 2026-03-18

### Added
- Accept `P0`–`P4` format for `--priority` flag (in addition to numeric)

### Fixed
- Verify-on-claim ignoring `--pass-ok` when `fail_first=false`

### Changed
- Improved `bn run` progress output and updated docs terminology
- Tightened `chrono` and `regex` version floors above known CVEs
- README examples diversified beyond auth domain
- Replaced comparison chart with Spec Kit, GSD, and Ralph loop sections

## [0.2.0] - 2026-03-01

### Added
- File locking to prevent concurrent agent writes
- Atomic file writes for crash safety
- CONTRIBUTING.md
- **Agent orchestration** — `bn run` dispatches ready beans to agents with ready-queue scheduling
- **Loop mode** — `bn run --loop-mode` continuously dispatches until no work remains
- **Auto-planning** — `bn run --auto-plan` decomposes large beans before dispatch
- **Adversarial review** — `bn run --review` spawns a second agent to verify correctness
- **Agent monitoring** — `bn agents` and `bn logs` for observing running agents
- **Memory system** — `bn fact` for verified project knowledge with TTL and staleness detection
- **Memory context** — `bn context` (no args) outputs stale facts, in-progress beans, recent completions
- **MCP server** — `bn mcp serve` for IDE integration (Cursor, Windsurf, Claude Desktop, Cline)
- **Library API** — `lib.rs` module with core type re-exports for use as a Rust crate
- **Interactive wizard** — `bn create` with no args launches step-by-step prompts (fuzzy parent search, smart verify suggestions, `$EDITOR` for descriptions)
- **Sequential chaining** — `bn create next` auto-depends on the most recently created bean
- **Trace command** — `bn trace` walks bean lineage, dependencies, artifacts, and attempt history
- **Recall command** — `bn recall` searches beans by keyword across open and archived beans
- **Pipe-friendly output** — `--json`, `--ids`, `--format` on list/show/verify/context commands
- **Stdin input** — `--description -`, `--notes -`, `--stdin` for batch operations
- **Batch close** — `bn close --stdin` reads IDs from stdin
- **Failure escalation** — `--on-fail "retry:3"` and `--on-fail "escalate:P0"` for verify failures
- **Config inheritance** — `extends` field for shared config across projects
- **Shell completions** — `bn completions` for bash, zsh, fish, and PowerShell
- **Agent presets** — `bn init --agent` with presets for Claude, pi, and aider
- **File context extraction** — `bn context <id>` extracts files referenced in bean descriptions
- **Structure-only context** — `bn context --structure-only` for signatures and imports only
- **Unarchive** — `bn unarchive` restores archived beans
- **Lock management** — `bn locks` views and clears file locks
- **Quick create** — `bn quick` creates and claims a bean in one step
- **Status overview** — `bn status` shows claimed, ready, and blocked beans
- **Context command** — `bn context` assembles file context from bean descriptions
- **Edit command** — `bn edit` opens beans in `$EDITOR` with schema validation and backup/rollback
- **Hook system** — pre-close hooks with `bn trust` for managing hook execution
- **Smart selectors** — `@latest`, `@blocked`, `@parent`, `@me` resolve to bean IDs dynamically
- **Verify-as-spec** — beans without a verify command are treated as goals, not tasks
- **Auto-suggest verify** — detects project type (Cargo.toml, package.json) and suggests verify commands
- **Fail-first enforcement** — verify must fail on create (on by default), `--pass-ok` to skip
- **Agent liveness** — `bn status` shows whether claimed beans have active agents
- **Better failure feedback** — verify failures show actionable output
- **Acceptance criteria** — `--acceptance` field for human-readable done conditions
- **Core CLI** — `bn init`, `bn create`, `bn show`, `bn list`, `bn close`
- **Verification gates** — every bean has a verify command that must pass to close
- **Hierarchical tasks** — dot notation (`3.1` is a child of `3`), `bn tree` for visualization
- **Smart dependencies** — `produces`/`requires` fields with auto-inference and cycle detection
- **Dependency graph** — `bn graph` with ASCII, Mermaid, and DOT output
- **Task lifecycle** — `bn claim`, `bn close`, `bn reopen`, `bn delete`
- **Failure tracking** — attempts counter, failure output appended to bean notes
- **Ready/blocked queries** — `bn ready` and `bn blocked` filter by dependency state
- **Dependency management** — `bn dep add/remove/list/tree/cycles`
- **Index engine** — cached index with `bn sync` for rebuild and `bn doctor` for health checks
- **Project stats** — `bn stats` for bean counts and status breakdown
- **Tidy command** — `bn tidy` archives closed beans, releases stale claims, rebuilds index
- **Markdown format** — beans stored as `.md` files with YAML frontmatter
- **Slug-based filenames** — `{id}-{slug}.md` naming convention
- **Archive system** — closed beans auto-archive to `.beans/archive/YYYY/MM/`
- **Git-native** — all state in `.beans/` directory, clean diffs, works offline

### Changed
- Improved robustness for parallel agent workflows
- Package renamed from `bn` to `beans-cli` for crates.io publication
- Improved help text and README for all current commands
- Improved `bn show` rendering with better formatting
- README rewritten with table of contents and consolidated documentation

### Removed
- `bn ready` — use `bn status` (shows ready beans in the Ready section, `--json` for scripting)
- `bn blocked` — use `bn status` (shows blocked beans in the Blocked section)
- `bn dep tree` — use `bn graph` (richer output with ASCII, Mermaid, DOT formats)
- `bn dep cycles` — use `bn doctor` (runs cycle detection among other health checks)

### Fixed
- `bn context` crash on corrupt archive YAML
- Missing `rules_file` and `memory` fields in test struct literals
- Shell escaping in verify commands
- File extension preservation during archiving
- `.md` format support in dep and verify commands

[Unreleased]: https://github.com/kfcafe/beans/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/kfcafe/beans/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/kfcafe/beans/releases/tag/v0.2.0
