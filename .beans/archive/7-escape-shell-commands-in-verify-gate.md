---
id: 7
title: Escape shell commands in verify gate
status: closed
priority: 0
created_at: |-
  2026-01-30T18:41:53.157385Z
updated_at: |-
  2026-01-30T18:51:07.517469Z
labels:
  - security
  - core
closed_at: |-
  2026-01-30T18:51:07.517469Z
verify: |-
  cargo test --lib commands::close
---

Fix shell command injection vulnerability in src/commands/close.rs.

Currently verify command from bean YAML is executed directly:
  let output = std::process::Command::new("sh")
    .arg("-c")
    .arg(&verify_cmd)  // <-- User input, no escaping

A bean with verify: "echo test; rm -rf ." would execute arbitrary code.

## Solution
Use proper shell escaping via shell_escape crate:
- Add to Cargo.toml: shell_escape = "0.1"
- Wrap verify_cmd: let escaped = shell_escape::escape(verify_cmd.into());
- Pass escaped version to shell

## Acceptance Criteria
- Verify commands properly shell-escaped before execution
- Existing verify tests still pass
- New test covers commands with shell metacharacters (; | & etc)
- Shell metacharacters safely escaped, not executed
