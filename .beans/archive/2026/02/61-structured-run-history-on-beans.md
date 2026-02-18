---
id: '61'
title: Structured run history on beans
slug: structured-run-history-on-beans
status: closed
priority: 1
created_at: 2026-02-22T07:45:07.089244Z
updated_at: 2026-02-22T08:39:53.728943Z
closed_at: 2026-02-22T08:39:53.728943Z
close_reason: 'Auto-closed: all children completed'
verify: cargo test history
is_archived: true
tokens: 23997
tokens_updated: 2026-02-22T07:45:07.091798Z
---

## Goal
Add structured run history to beans so every attempt is recorded with timing, cost, agent identity, and outcome. Inspired by Visor's OpenTelemetry tracing but stored directly on the bean markdown file.

## Motivation
Currently failure output is appended as markdown text in the notes field. This is unstructured — you can't query 'how many tokens did this feature cost?' or 'what was the average attempt duration?'. deli already HAS this data (it tracks tokens, cost, timing per agent) but it's in ephemeral log files.

## What to Build

### 1. RunRecord struct
```rust
pub struct RunRecord {
    pub attempt: u32,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub duration_secs: Option<f64>,
    pub agent: Option<String>,       // who ran it (pi-abc123, user, bw)
    pub result: RunResult,           // pass, fail, timeout, cancelled
    pub exit_code: Option<i32>,
    pub tokens: Option<u64>,
    pub cost: Option<f64>,
    pub output_snippet: Option<String>, // first/last N lines of output
}

pub enum RunResult {
    Pass,
    Fail,
    Timeout,
    Cancelled,
}
```

### 2. Add history field to Bean
- `pub history: Vec<RunRecord>` on Bean struct
- Serialize/deserialize in YAML frontmatter
- Skip when empty

### 3. Record history on close attempts  
- In cmd_close, before/after running verify, create a RunRecord
- Replace the current 'append failure to notes' with structured history entry
- Keep backward compat: still show failure info in bn show output

### 4. bn show integration
- Display history in `bn show` output: table of attempts with timing/result
- Add `--history` flag for detailed view

### 5. bn tree --cost (stretch)
- Sum tokens/cost across a subtree
- Display totals in tree output

## Files
- src/bean.rs (RunRecord, RunResult, history field)
- src/commands/close.rs (record history on verify attempts)
- src/commands/show.rs (display history)
- src/commands/tree.rs (optional: --cost flag)

## Edge Cases
- Migrate existing notes-based failure records? Probably not — just start fresh
- History should not bloat bean files — cap at max_attempts entries
- RunRecord timestamps should use UTC consistently
