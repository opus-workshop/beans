id: '16'
title: Pi Agent Tool Extensions
slug: pi-agent-tool-extensions
status: closed
priority: 2
created_at: 2026-02-02T09:23:10.476073Z
updated_at: 2026-02-02T09:37:55.185002Z
description: |-
  Parent bean for custom pi extension tools that give agents better capabilities. Each child implements a specific tool as a pi extension.

  ## Tools to Build
  1. **Persistent Terminal (tmux)** - Long-running processes, dev servers, REPLs
  2. **LSP Navigation** - Go-to-definition, find-references via lsproxy
  3. **Database Query** - Direct SQL execution with structured results
  4. **Structured Test Runner** - Run tests, parse pass/fail per test
  5. **Diff Tool** - Generate/apply unified diffs
  6. **File Watcher** - Get notified when files change

  ## Research Found
  - tmux-mcp-server: github.com/lox/tmux-mcp-server (Go, can adapt pattern)
  - lsproxy: github.com/agentic-labs/lsproxy (Docker container with REST API)
  - Various MCP database servers exist for postgres/sqlite

  ## Architecture
  Each tool is a pi extension in ~/.pi/agent/extensions/ following the pattern in docs/extensions.md
closed_at: 2026-02-02T09:37:55.185002Z
verify: test -d ~/.pi/agent/extensions/terminal && test -d ~/.pi/agent/extensions/diff && test -d ~/.pi/agent/extensions/file-watcher && test -d ~/.pi/agent/extensions/lsp && test -d ~/.pi/agent/extensions/database && test -d ~/.pi/agent/extensions/test-runner
is_archived: true
