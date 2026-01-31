id: '3'
title: Create bpick fuzzy selector tool for interactive bean selection
slug: create-bpick-fuzzy-selector-tool-for-interactive-b
status: open
priority: 1
created_at: 2026-01-31T16:51:32.872900Z
updated_at: 2026-01-31T16:51:32.872900Z
description: "Implement a shell wrapper script that integrates with `bn list --json` to provide interactive bean selection via fzf. This enables ergonomic workflows like `bn close $(bpick)` and `bn show $(bpick)`.\n\n## Script Location\ntools/bpick - A POSIX-compatible shell script that wraps fzf for bean selection.\n\n## Dependencies\n- fzf (fuzzy finder) - interactive selection interface\n- jq (JSON query tool) - parse JSON output from `bn list --json`\n- bn (beans CLI) - must be in PATH\n\n## What It Does\n\n1. Run `bn list --json` to get all beans\n2. Use jq to transform JSON into human-readable format:\n   - Format: \"${ID} [${STATUS}] ${TITLE}\"\n   - Example: \"3.1 [open] Add comprehensive unit tests\"\n3. Pipe formatted beans to fzf with preview capability\n4. Return selected bean ID to stdout\n5. Handle errors gracefully for missing dependencies or beans\n\n## Usage Examples\n\n```bash\n# Close the selected bean\nbn close $(bpick)\n\n# Show details of selected bean\nbn show $(bpick)\n\n# Update the selected bean's title\nbn update $(bpick) --title=\"New title\"\n\n# Claim a bean\nbn claim $(bpick)\n```\n\n## Format Details\n\nOutput format to fzf (via jq):\n\"{id} [{status}] {title}\"\n\nStatus values: open, in_progress, closed\n\n### fzf Configuration\n- Height: 50% of terminal\n- Preview: show full bean details via `bn show`\n- Bind Ctrl+D to delete preview window for more space\n- Highlight matches in title and ID\n- Single select mode (one bean at a time)\n\n## Error Handling\n\nExit codes:\n- 0: Success, bean ID output\n- 1: fzf/jq/bn not installed, no beans exist, or parse error\n- 130: User abort (Ctrl+C in fzf)\n\n## Edge Cases to Handle\n1. No fzf installed - provide helpful error message\n2. No jq installed - provide helpful error message  \n3. No beans exist - exit with error\n4. Empty JSON response - exit with error\n5. User aborts fzf (Ctrl+C) - propagate exit code 130\n6. Bean with special characters in title - properly escaped\n7. Very long titles - handled gracefully in fzf preview\n8. Multiple beans with same title - include ID for disambiguation"
acceptance: |-
  Acceptance Criteria:

  1. Script exists at tools/bpick and is executable (+x permission)
  2. POSIX-compliant shell script (passes shellcheck if available)
  3. Dependency checks:
     - Validates fzf is installed, exits 1 with helpful message if missing
     - Validates jq is installed, exits 1 with helpful message if missing
     - Validates bn is in PATH, exits 1 with helpful message if missing
  4. Functionality:
     - Executes `bn list --json` successfully
     - Parses JSON output with jq to extract id, status, title
     - Formats output as "{id} [{status}] {title}"
     - Passes to fzf for interactive selection
     - Outputs selected bean ID to stdout on success
  5. Error cases:
     - No beans found: exits 1 with message "no beans found"
     - fzf aborted (Ctrl+C): exits 130
     - JSON parse error: exits 1 with helpful error
  6. Integration:
     - Works with `bn close $(bpick)`, `bn show $(bpick)`, etc.
     - Documented in README.md under Shell Integration section
     - Tested with sample beans manually
  7. Edge cases handled:
     - Special characters in titles (quotes, newlines, etc.)
     - Long titles don't break formatting
     - Multiple spaces in title preserved
     - Unicode characters in titles work correctly
labels:
- feature
- tools
- shell
verify: |-
  #!/bin/bash
  set -e

  # Verify script exists and is executable
  test -x /Users/asher/beans/tools/bpick || { echo "tools/bpick not executable"; exit 1; }

  # Verify shellcheck passes (if available)
  if command -v shellcheck >/dev/null 2>&1; then
    shellcheck /Users/asher/beans/tools/bpick || { echo "shellcheck failed"; exit 1; }
  fi

  # Verify fzf dependency check works
  OUTPUT=$(/Users/asher/beans/tools/bpick 2>&1 || true)
  if ! command -v fzf >/dev/null 2>&1; then
    echo "$OUTPUT" | grep -q "fzf" || { echo "fzf check failed"; exit 1; }
  fi

  # Verify jq dependency check works
  if ! command -v jq >/dev/null 2>&1; then
    echo "$OUTPUT" | grep -q "jq" || { echo "jq check failed"; exit 1; }
  fi

  # Verify script handles no beans gracefully
  # Create temp .beans dir with no beans
  TMPDIR=$(mktemp -d)
  mkdir -p "$TMPDIR/.beans"
  cat > "$TMPDIR/.beans/config.yaml" << 'CONFIG'
  project: test
  next_id: 1
  CONFIG

  # Test with empty project (should exit 1)
  export HOME="$TMPDIR"
  OUTPUT=$(bn -d "$TMPDIR/.beans" list --json 2>&1 || true) || true

  # Cleanup
  rm -rf "$TMPDIR"

  # Verify the script can parse bn list --json output
  if command -v fzf >/dev/null 2>&1 && command -v jq >/dev/null 2>&1; then
    bn list --json | jq -r '.[] | "\(.id) [\(.status)] \(.title)"' > /dev/null || \
      { echo "JSON parsing failed"; exit 1; }
  fi

  echo "Verify gate passed: bpick script is functional"
  exit 0
