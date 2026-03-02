---
id: '113'
title: Add rust-version MSRV to Cargo.toml
slug: add-rust-version-msrv-to-cargotoml
status: closed
priority: 2
created_at: '2026-03-02T02:27:57.310124Z'
updated_at: '2026-03-02T02:28:30.531018Z'
labels:
- chore
- publish-prep
closed_at: '2026-03-02T02:28:30.531018Z'
verify: cd /Users/asher/beans && grep -q '^rust-version' Cargo.toml
fail_first: true
checkpoint: '655ee759245f4c916a5cb00eff3a627366a732b7'
claimed_by: pi-agent
claimed_at: '2026-03-02T02:28:05.043223Z'
is_archived: true
tokens: 434
tokens_updated: '2026-03-02T02:27:57.311024Z'
history:
- attempt: 1
  started_at: '2026-03-02T02:28:30.532578Z'
  finished_at: '2026-03-02T02:28:30.587515Z'
  duration_secs: 0.054
  result: pass
  exit_code: 0
attempt_log:
- num: 1
  outcome: success
  agent: pi-agent
  started_at: '2026-03-02T02:28:05.043223Z'
  finished_at: '2026-03-02T02:28:30.531018Z'
---

## Task
Add a `rust-version` field to `Cargo.toml` under `[package]` to declare the MSRV (minimum supported Rust version).

## Steps
1. Add `rust-version = "1.70"` to `Cargo.toml` in the `[package]` section, after `edition`.
2. Verify with `cargo check`

## Context
Without this field, users on older Rust versions get confusing compile errors instead of a clean "requires Rust 1.70+" message from cargo.

## Don't
- Don't set it too high (like 1.80) — aim for the oldest version that actually compiles
- Don't set it too low and risk breakage — 1.70 is a safe bet for edition 2021 crates with modern deps
