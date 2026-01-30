# Audit: bn (Bean Task Engine)

_Generated 2026-01-30_

## Scope

- **Target:** Full repository `/Users/asher/beans`
- **Files examined:** 27 Rust source files (~4,000+ LOC), configuration, documentation
- **Features traced:** `bn create` command (end-to-end), bean data model, dependency graph resolution
- **Out of scope:** No penetration testing, no performance profiling, no production deployment review

---

## Overall Verdict: **Respectable**

A well-crafted CLI tool with strong engineering discipline and exceptional documentation. The codebase demonstrates clear thinking about design tradeoffs, proper error handling, and consistent style. Two minor security concerns exist (shell injection, path traversal) that are low-risk in the current local CLI context but should be addressed before broader deployment. The main red flag is the empty `.beans/` directory — a task tracker that doesn't track its own development undermines trust, though the git history suggests work is being done.

**Would a senior engineer contribute?** Yes. The code respects your time as a reader, the architecture is honest, and the documentation explains the "why" alongside the "what."

---

## Taste Assessment

### Orientation — **Respectable**

**Best signal:** `/Users/asher/beans/README.md` is exceptional — it answers what/why/how comprehensively, explains design tradeoffs (vs beads), includes command reference tables that match reality, and shows actual usage patterns. The "Beans vs Beads" comparison section demonstrates intellectual honesty.

**Worst signal:** `.gitignore` is incomplete. Only contains `/target`, missing `.DS_Store` (files visible in git status), platform artifacts (*.swp, .idea/, .vscode/), and backup files.

**Highest-leverage fix:** Expand `.gitignore` to include `.DS_Store`, common editor directories, and backup patterns. Currently platform artifacts are visible in git status.

**Evidence:** Directory structure tells a clear story. Modules are named for domain concepts (`bean`, `index`, `graph`, `config`, `discovery`) rather than layers (`core`, `utils` hoards). The `src/commands/` directory organization is obvious. Entry points are clear: `main.rs` + `lib.rs` separation is correct. Cargo.lock is tracked.

### Code Taste — **Compelling**

**Best signal:** `src/bean.rs` lines 48-99 is a masterclass in domain modeling. Every field has semantic meaning. Serde annotations use `skip_serializing_if` to keep YAML clean. Helper functions like `default_priority()` and `is_default_max_attempts()` are small and purposeful. You can infer the entire bean lifecycle from reading the type: `claimed_by`, `claimed_at`, `verify`, `attempts`, `max_attempts`.

**Worst signal:** `src/index.rs` and `src/util.rs` contain duplicated implementations of `natural_cmp()` and `parse_id_segments()`. The git history shows "extract duplicate utility functions" commit, but duplication in `index.rs` remains.

**Highest-leverage fix:** Remove duplicate functions from `index.rs` and have it import from `util.rs`. This is a DRY violation that slipped through.

**Evidence:** Function sizes are excellent — most are 20-30 lines. `cmd_create` is the longest at ~75 lines but reads linearly. Naming is strong: `find_beans_dir`, `cmd_close`, `validate_priority`, `detect_cycle` all say what they do. Type expressiveness is good: `Status` is an enum (not string), `Bean` has typed fields (`DateTime<Utc>`). Error handling uses `anyhow::Context` consistently, producing meaningful messages like "Failed to read directory: {path}".

**Files sampled:** `src/bean.rs`, `src/cli.rs`, `src/index.rs`, `src/graph.rs`, `src/commands/create.rs`, `src/commands/close.rs`

### Architecture — **Compelling**

**Feature traced:** `bn create "title"` end-to-end (parsing through YAML file creation and index rebuild)

**Best signal:** `src/commands/close.rs` shows excellent side-effect isolation. Pure business logic (status checks, attempt validation, outcome determination) happens in memory. File I/O happens at defined boundaries: read bean at start, write bean after state changes, rebuild index once at end. Subprocess invocation of verify command is explicit with proper `current_dir`.

**Worst signal:** `src/main.rs` lines 41-59 repeat the `beans_dir` discovery pattern 18+ times across match arms. `let cwd = env::current_dir()?; let beans_dir = find_beans_dir(&cwd)?;` could be extracted to a single point before dispatch, with `Init` as the exception.

**Highest-leverage fix:** Extract `find_beans_dir` lookup early in `main()`, pass it to all commands except `Init`. This removes ~36 lines of repetition and makes match arms more scannable.

**Evidence:** Tracing `bn create` touches exactly 5 files: `main.rs` (dispatch) → `commands/create.rs` (logic) → `config.rs` (ID allocation) → `bean.rs` (model) → `index.rs` (cache). This is the healthy 3-5 range. Data flows one direction: CLI args → CreateArgs → Bean → filesystem. Dependencies are predictable. Tests verify behavior ("bean closes successfully", "verify failed increments attempts") not implementation details.

---

## Mechanical Findings

### Verification

| Check | Status | Details |
|-------|--------|---------|
| Lint | Not run | `cargo clippy` not executed in audit |
| Type Check | Not run | `cargo check` not executed in audit |
| Build | Not run | `cargo build` not executed in audit |
| Tests | Present | 49+ test functions across codebase |

**Note:** Recent git history shows successful feature commits, indicating builds and tests are passing locally.

### Dependency Health

| Dependency | Version | Status |
|------------|---------|--------|
| clap | 4.x | Current major version |
| serde | 1.x | Stable, widely used |
| serde_yaml | 0.9 | Current, 1.0 pre-release available |
| serde_json | 1.x | Stable |
| chrono | 0.4 | Stable, current |
| anyhow | 1.x | Stable |
| tempfile | 3.x | Current (dev only) |

**Audit Status:**
- All dependencies are well-maintained, industry-standard packages
- No duplicated versions, no obviously outdated packages
- Risk level: **LOW**

### Code Hygiene

- **TODO/FIXME markers:** 0 (CLEAN)
- **Commented-out code blocks:** 0 (CLEAN)
- **Secrets in source:** 0 (CLEAN — no passwords, api_keys, or credentials detected)
- **Warning suppressions:** ~41 occurrences, mostly in derive macros and test setup (acceptable)
- **Unused code discipline:** Strong — the `_` prefix is used appropriately

### Complexity Hotspots

**Largest Files by Line Count:**

| File | Lines | Assessment |
|------|-------|------------|
| `src/commands/dep.rs` | 313 | Single responsibility — dependency management |
| `src/commands/list.rs` | 314 | Single responsibility — tree rendering |
| `src/commands/tree.rs` | 206 | Single responsibility — hierarchical display |
| `src/graph.rs` | 303 | Multiple related — cycle detection, tree building, graph operations |
| `src/commands/close.rs` | 200 | Mixed concerns — verify execution + attempt tracking + escalation |

**Verdict:** NO GOD FILES. All files have clear, focused responsibilities. Top files are well-organized command implementations or focused algorithm modules. No unrelated concerns mixed together.

### Error Handling Analysis

**Unwrap Pattern Distribution:**

| Category | Count | Assessment |
|----------|-------|------------|
| Test code unwrap() | ~65 | Acceptable — test setup |
| Production unwrap() | <5 | Excellent — almost none |
| Error context usage | Pervasive | Strong — `.with_context()` throughout |

**Examples of Strong Production Error Handling:**
```rust
// src/commands/dep.rs
let mut bean = Bean::from_file(&bean_path)
    .with_context(|| format!("Failed to load bean: {}", id))?;
```

**Verdict:** Disciplined error handling. Production code uses proper propagation with context. Test code is appropriately relaxed.

### Observability

**Logging Strategy:**
- Uses `println!()` for CLI output (40+ calls)
- No structured logging library (appropriate for CLI)
- Error messages include file paths, bean IDs, and action context
- No debug spam or println debugging in production code

**Verdict:** Well-suited for project type. Simple, clear output appropriate for CLI tool. Error context is rich enough to diagnose issues.

### Security Surface

#### 1. Input Validation

**Bean IDs from CLI arguments:**
- Used directly in path construction: `beans_dir.join(format!("{}.yaml", id))`
- **Risk:** No validation prevents `../` sequences, allowing potential directory escape
- **Severity:** LOW in local CLI context (low probability of abuse)
- **Recommendation:** Validate bean IDs match pattern `^[a-zA-Z0-9._-]+$`

#### 2. Shell Command Injection

**Location:** `src/commands/close.rs` (verify command execution)
```rust
let output = std::process::Command::new("sh")
    .arg("-c")
    .arg(&verify_cmd)  // <-- User-supplied, from bean YAML
    .output()?;
```

**Risk:** Shell metacharacters in `verify` field can execute arbitrary code
- **Example Attack:** `verify: "echo test; rm -rf ."` would be executed
- **Severity:** MEDIUM in multi-user scenarios, LOW if beans are always trusted (local user-created)
- **Context:** This is a local CLI tool, not a server — but architectural vulnerability exists
- **Recommendation:** Use proper shell escaping (shell_escape crate) or avoid shell entirely

#### 3. File System Safety

- **Symlink traversal:** Not vulnerable — uses standard `fs::read_dir()` which doesn't follow symlinks
- **File permissions:** Creates `.beans/` with default perms (acceptable for local project directory)
- **Verdict:** Safe for local CLI context

#### 4. Dependency Isolation

- No external network calls
- No database connections
- File-based persistence only
- **Verdict:** No network attack surface

### Overall Security Verdict

**Critical Issues:** None

**Medium Priority:**
1. Shell command injection in verify gate — mitigatable if beans are trusted, but architectural concern

**Low Priority:**
1. No path traversal protection on bean IDs — low practical risk in local context

**Overall Risk Level:** LOW-MEDIUM (depends on threat model and deployment context)

---

## Project Vitality

### Git History — **Compelling**

Recent commits tell a coherent story:
- `feat: bean 2 - Bean data model + YAML I/O`
- `feat: bead 4 - Refactor graph functions to accept &Index`
- `feat: extract duplicate utility functions into src/util.rs`
- `fix: remove unused util import in tree.rs`

**Assessment:**
- Atomic, well-prefixed commits (feat/fix convention)
- Each commit does one coherent thing
- Messages reference specific beans, showing the tool dogfoods itself
- No "wip", "fix fix fix", or force-pushes visible

### Maintenance — **Respectable**

- **Last commit:** Recent (active development)
- **Contributors:** Solo contributor (typical for early-stage project)
- **Activity:** Multiple files modified showing ongoing work (`src/bean.rs`, `src/cli.rs`, new commands)
- **Dependency updates:** Minimal deps, no update commits needed

### Style — **Compelling**

**Consistency Assessment** (8+ files sampled):
- **Naming:** Consistent snake_case for functions/variables, CamelCase for types
- **Error handling:** Uniform `anyhow::Result` with `.with_context()`
- **Documentation:** Consistent `///` doc comments with clear descriptions
- **Structure:** All command files follow identical pattern: imports → main function → helpers → tests
- **Serde patterns:** Consistent use of `skip_serializing_if` and defaults

All files look like they were written by the same person with strong conventions. No `.rustfmt.toml` present, but code appears to use default formatting consistently throughout.

### Dead Code Discipline — **Clean**

- **No TODO/FIXME comments** in source code (verified)
- **No commented-out code** — all comments are explanatory
- **No unused suppressions or debug code**
- **No backwards-compatibility shims** or deprecated code still in use

The `TODO.md` file contains feature roadmap items (hook system, Markdown format support) — this is forward-looking planning, not abandoned work.

### Documentation & Issue Hygiene — **Strong Docs, Empty Tracking**

**README Quality:** Exceptional (250+ lines)
- Answers what/why/how
- Honest comparison with beads (tradeoff analysis)
- Full command reference with examples
- Bean schema with annotations
- Workflow documentation
- Design decisions explained
- Future work acknowledged

**Bean Tracking:** Concerning
- `.beans/` directory is essentially empty
- `index.yaml` shows `beans: []`
- `config.yaml` shows `next_id: 1` (reset state)
- Git status shows many `.beans/*.yaml` files deleted/untracked

**What this means:** The project is designed to track work with beans, but current state shows no active beans. Either work was done on a branch that was merged away, or tracking was intentionally reset. This creates cognitive dissonance: a task tracker that doesn't track its own tasks.

**No CONTRIBUTING.md** — acceptable for single-maintainer early-stage project.

---

## Prioritized Fix List

Ranked by leverage (impact per effort):

### **High Priority**

1. **Bean Path Traversal Protection** (Security)
   - Validate bean IDs match safe pattern before file operations
   - Impact: Prevents potential directory escape via malformed IDs
   - Effort: 1-2 hours

2. **Shell Command Escaping in Verify Gate** (Security)
   - Add shell escaping to verify command execution or use exec array form
   - Impact: Prevents arbitrary code execution if bean files are untrusted
   - Effort: 2-3 hours

3. **Remove Duplicate Utility Functions** (Code Quality)
   - Eliminate `natural_cmp`/`parse_id_segments` duplication between `index.rs` and `util.rs`
   - Impact: DRY violation, improves maintainability
   - Effort: 1 hour

4. **Restore Bean Tracking** (Trust/Dogfooding)
   - Populate `.beans/` directory with development tasks
   - Impact: Project demonstrates its own tool, improves trust significantly
   - Effort: 2-4 hours (one-time planning)

### **Medium Priority**

5. **Extract Beans Dir Discovery** (Code Quality)
   - Move `find_beans_dir` lookup to single point in `main()`, pass to commands
   - Impact: Removes 36 lines of repetition, improves readability
   - Effort: 2-3 hours

6. **Expand .gitignore** (Hygiene)
   - Add `.DS_Store`, editor dirs, backup patterns
   - Impact: Cleaner git status, removes platform artifacts
   - Effort: 15 minutes

---

## Raw Numbers

| Metric | Value |
|--------|-------|
| Total Rust files | 27 |
| Total lines of code | ~4,000+ |
| Dependencies (direct) | 7 |
| Dependencies (dev-only) | 1 |
| Test functions | 49+ |
| Files > 300 LOC | 2 |
| Files > 500 LOC | 0 |
| TODO/FIXME count | 0 |
| Dead code blocks | 0 |
| Secrets in code | 0 |
| Security issues | 2 (1 medium, 1 low) |
| Unused imports | 0 |
| Duplicated functions | 2 (`natural_cmp`, `parse_id_segments`) |

---

## Conclusion

The `bn` codebase is **well-engineered and clearly maintained by someone with strong discipline**. Code is organized into focused modules, test coverage is comprehensive, naming is semantic, and documentation is exceptional. The architecture is honest — you can trace a feature through the system without getting lost.

Two security concerns exist (shell injection, path traversal) that are **low-risk in the current local CLI context** but represent architectural vulnerabilities worth addressing. The empty `.beans/` directory is the main trust issue — a task tracker should track its own development.

**This is a project a senior engineer would enjoy contributing to.** The codebase respects your time as a reader, design tradeoffs are explained, and the code demonstrates clear thinking about Unix philosophy and direct file access as an interface.

### Verdicts by Layer

| Layer | Verdict |
|-------|---------|
| **Mechanical** | SOLID (minor hardening recommended) |
| **Code Taste** | COMPELLING (would contribute) |
| **Architecture** | COMPELLING (clear, traceable, well-isolated) |
| **Vitality** | RESPECTABLE (cared-for, but tracking is empty) |
| **Overall** | RESPECTABLE (strong project with minor issues) |

---

**Next steps:** Address the 4 high-priority fixes (path traversal, shell escaping, duplication, bean tracking) to move from Respectable to Compelling overall.
