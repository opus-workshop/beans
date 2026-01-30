,.....................jkmTo strictly adhere to the Unix philosophy (**"Expect the output of every program to become the input to another, as yet unknown, program"**), `beans` should remain a specialized text manipulator.

However, to make it a true power tool for hackers and engineers, I would build a surrounding "coreutils" ecosystem and add specific piping features to the main binary.

Here is the **Beanstalk Toolchain**.

---

### 1. New Features for the Core Binary (`bn`)

To make `bn` play nicer in pipes, I would add:

**A. Formatted Output (Template Engine)**
Instead of just `--json` or `--short`, add Go-template or Rust Handlebars style formatting.
```bash
# Extract just the file paths mentioned in a bean's description
bn show beans-3.2 --format '{{ range .files }}{{ .path }}{{ "\n" }}{{ end }}'
```

**B. "Smart Selectors" (The `@` Syntax)**
Allow relative referencing so you don't need to memorize IDs.
*   `bn show @latest` (Newest created bean)
*   `bn show @blocked` (All currently blocked beans)
*   `bn close @parent` (Close the parent of the current bean—useful in scripts)
*   `bn update @me --assignee $(whoami)`

**C. `bn edit` (The `$EDITOR` shim)**
Does not modify the file itself, but opens the YAML file in `$EDITOR`. When the editor closes, it validates the schema. If invalid, it prompts to re-edit.

---

### 2. The Companion Tools (The "Beanstalk")

I would build these as separate binaries or shell scripts.

#### `bpick` — The Fuzzy Selector
**Purpose:** Interactive selection using `fzf`.
**Philosophy:** Never type an ID manually.
**Implementation:**
```bash
#!/bin/bash
# Usage: bn close $(bpick)
bn list --all --json | \
jq -r '.[] | "\(.id)\t\(.status)\t\(.title)"' | \
fzf --height 40% --reverse --header="Select Bean" | \
cut -f1
```

#### `bctx` — The Context Assembler (Killer App for Agents)
**Purpose:** Preparing the "packet" for an LLM/Agent.
**Function:** It reads a bean, finds every file path mentioned in the `description` or `files` array, and concatenates the *content* of those files into a single prompt block.
**Usage:**
```bash
# "Give me the prompt for bean 3.2, including the source code it references"
bctx beans-3.2 | llm "Implement this"
```
**Why:** This solves the "cold start" problem for agents. The bean becomes a manifest for the context window.

#### `bmake` — Dependency-Aware Execution
**Purpose:** Execute commands only when the DAG permits.
**Function:** A wrapper that checks `bn blocked`.
**Usage:**
```bash
# Only runs the deploy script if the 'Security Review' bean is closed
bmake beans-50 "./deploy.sh"
```
If `beans-50` is blocked or open, it exits with status 1. This turns your task tracker into a CI/CD gatekeeper.

#### `btime` — The Punch Clock
**Purpose:** Calculate cycle time.
**Function:** Hooks into `git` to analyze the delta between a bean's `created_at` and `closed_at`, or tracks active time based on `status: in_progress` transitions in the git log.
**Output:** "Bean 3.2 took 4 hours."

---

### 3. "Hacker" Integrations

#### The Git Commit Hook (`prepare-commit-msg`)
Automatically prepend the Bean ID to commit messages if you are working on a branch named after the bean.
*   **Branch:** `feat/beans-3.2-list-command`
*   **User types:** `git commit -m "Added sorting"`
*   **Hook makes it:** `git commit -m "[beans-3.2] Added sorting"`
*   **Why:** `beans` can later scan `git log` to auto-fill the `notes` section of the bean with the commit hashes and messages associated with it.

#### `bgrep` — Semantic Grep
Since beans are flat files, you can grep them. But `bgrep` adds structure.
```bash
# Find all beans where "database" appears in the description 
# BUT the status is NOT closed.
bgrep "database" --field description --status open
```

#### `bviz` — TUI Dashboard
A specialized TUI (using `ratatui` in Rust) that visualizes the DAG.
*   **Left pane:** Tree view of beans.
*   **Right pane:** Markdown renderer for the selected bean description.
*   **Bottom pane:** Dependency graph.
*   **Keybinds:** `Space` to toggle status, `Enter` to edit.

---

### 4. The "Protocol" Extension

If I were extending the ecosystem, I would add a **Bean Server Protocol (BSP)**.

Since `beans` is designed to be agent-executable, creating a standard JSON-RPC interface (over Stdin/Stdout) would allow IDEs (VS Code, Neovim) and Agent Runtimes (LangChain, AutoGPT) to natively "speak" beans without spawning the CLI repeatedly.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "method": "bean/propose_split",
  "params": { "id": "beans-3", "strategy": "by_file_type" }
}Since `beans` is stateless and file-based, "hooks" cannot be event listeners waiting on a port. They must be **synchronous shell scripts** executed by the binary itself at specific lifecycle points.

This transforms `beans` from a passive text store into an **Active Workflow Engine**.

Here is the design for the **Beans Hook System**.

### 1. The Structure
Hooks live in the hidden project directory alongside the config.

```text
.beans/
  hooks/
    pre-create       # Runs before a bean is written to disk
    post-create      # Runs after successful creation
    pre-update       # Runs before modification (validation)
    post-update      # Runs after modification (triggers)
    pre-close        # The "Gatekeeper"
    post-merge       # (Git hook) Runs after 'git pull' brings in new beans
```

### 2. The Interface
When `bn` triggers a hook, it should pipe the **context** as JSON into the script's `stdin`.

**Example Payload (for `pre-update`):**
```json
{
  "event": "update",
  "bean_id": "beans-3.2",
  "actor": "user_or_agent_name",
  "changes": {
    "status": { "old": "open", "new": "closed" }
  },
  "full_bean": { ... }
}
```

The hook script exits with:
*   `0`: Proceed.
*   `Non-zero`: Abort operation (the `bn` command fails and prints the hook's `stderr`).

### 3. Use Case Examples

#### A. The "Policy Enforcer" (`pre-create`)
**Scenario:** Prevent anyone (human or agent) from creating a "Critical" (P0) task without assigning an owner immediately.

**File:** `.beans/hooks/pre-create`
```bash
#!/bin/bash
# Read JSON from stdin
input=$(cat)

priority=$(echo "$input" | jq '.bean.priority')
assignee=$(echo "$input" | jq -r '.bean.assignee')

if [ "$priority" -eq 0 ] && [ "$assignee" == "null" ]; then
  echo "Error: P0 beans must have an assignee immediately." >&2
  exit 1
fi

exit 0
```

#### B. The "CI Gatekeeper" (`pre-close`)
**Scenario:** You cannot close a bean if the tests referenced in it aren't passing.
**Logic:** The hook greps the bean description for test files, runs them, and blocks the `bn close` command if they fail.

**File:** `.beans/hooks/pre-close`
```bash
#!/bin/bash
# Extract test files mentioned in the description (e.g., "tests/foo.rs")
files=$(jq -r '.bean.description' | grep -oE "tests/[a-zA-Z0-9_]+\.rs")

if [ -z "$files" ]; then
  echo "Warning: No tests detected in description. Proceeding..." >&2
  exit 0
fi

echo "Running verification tests..." >&2
cargo test $files
if [ $? -ne 0 ]; then
  echo "Blocking close: Tests failed." >&2
  exit 1
fi
```

#### C. The "Swarm Signal" (`post-update`)
**Scenario:** When a bean is marked `blocked`, automatically summon a specialized "Unblocker Agent" (or ping a Slack channel).

**File:** `.beans/hooks/post-update`
```bash
#!/bin/bash
status=$(jq -r '.changes.status.new')
id=$(jq -r '.bean_id')

if [ "$status" == "blocked" ]; then
  # Theoretical CLI for an LLM agent
  swarm-cli invoke --agent "SeniorDev" \
    --prompt "Bean $id was just blocked. Read it and advise."
fi
```

#### D. The "Ghost Writer" (`post-create`)
**Scenario:** When a generic task is created, use an LLM to automatically fill in the `design` and `acceptance` criteria if they are empty.

**File:** `.beans/hooks/post-create`
```bash
#!/bin/bash
# If description is short/empty, auto-expand it
desc_len=$(jq -r '.bean.description | length')
id=$(jq -r '.bean.id')

if [ "$desc_len" -lt 50 ]; then
  echo "Auto-expanding context for $id..." >&2
  
  # Generate content
  new_desc=$(bctx $id | llm "Expand this task into a technical spec")
  
  # Write it back (careful to avoid infinite loops!)
  # We use a flag or environment variable to bypass hooks to prevent loops
  BN_NO_HOOKS=1 bn update $id --description "$new_desc"
fi
```

### 4. Git Hooks Integration
Since `beans` relies on `git`, you need to hook into `git` to handle changes that come from *other people*.

**File:** `.git/hooks/post-merge`
```bash
#!/bin/bash
# Detect if any .yaml files in .beans/ changed
changed_beans=$(git diff --name-only HEAD@{1} HEAD | grep ".beans/.*.yaml")

if [ -n "$changed_beans" ]; then
  echo "Syncing index..."
  bn sync
  
  # Check for newly unblocked items assigned to me
  bn ready --assignee $(whoami) | while read bean; do
    notify-send "Ready to work" "$bean is now unblocked."
  done
fi
```

### 5. Security Implications (The Hacker View)

If you are using `beans` in a shared repository:
1.  **Arbitrary Code Execution:** If you clone a repo and run `bn create`, and the repo contains a malicious `.beans/hooks/pre-create`, **you just executed their code.**
2.  **Mitigation:**
    *   `bn` should behave like `direnv` or `git`: It must explicitly ask for permission to run hooks in a new directory.
    *   `bn trust` command to whitelist the hooks directory.
    *   Otherwise, it prints: *"Skipping untrusted hooks. Run 'bn trust' to enable."*

### Summary
Hooks allow `beans` to act as the **Orchestrator**.
*   **Input:** File changes.
*   **Process:** Hook scripts.
*   **Output:** CI jobs, Agent invocations, Slack alerts, or Git commits.

This completes the Unix philosophy: `beans` manages the *state*, but hooks manage the *flow*.
```

This turns `beans` from a tool into a platform for autonomous software engineering.

The correct format for `beans` is **Markdown with YAML Frontmatter** (the format used by Jekyll, Hugo, and Obsidian).

Here is the breakdown of why you should switch from `.yaml` to `.md`.

### 1. The "Multi-line String" Problem
Your beans are agent prompts. They contain code snippets, design docs, and complex instructions.

**In YAML:**
You are forced to use the block scalar (`|`) and maintain strict indentation. If you paste a code snippet that has its own indentation, you risk breaking the YAML parser.

```yaml
# 3.2.yaml
description: |
  Implement the list command.
  Here is the struct:
    # IF YOU PASTE CODE HERE AND MESS UP INDENTATION
    # THE ENTIRE FILE BECOMES INVALID
    pub struct Bean {
        pub id: String,
    }
```

**In Markdown:**
You have native fences. The structure is separated from the content.

```markdown
---
id: beans-3.2
status: open
dependencies: [beans-2]
---

# Implement the list command

Here is the struct:

```rust
pub struct Bean {
    pub id: String,
}
```
```

### 2. The "Git Diff" Problem
`beans` relies on Git for history.

*   **YAML:** If you change a key near the top, or re-indent a block, the diff can look noisy.
*   **Markdown:** The metadata (Frontmatter) is distinct from the prose. Changes to the description show up as standard text diffs, which is exactly what you want when refining an agent prompt.

### 3. The "Viewer" Ecosystem
This is the killer feature.

If you use **YAML**:
*   `cat 3.2.yaml` = Wall of text.
*   GitHub UI = Wall of text.
*   IDE = Syntax highlighting is okay, but code snippets inside the string block usually don't get highlighted.

If you use **Markdown**:
*   `cat 3.2.md` = Readable text (with `bat` or `glow`).
*   **GitHub UI** = Renders the task as a beautiful document. The metadata table renders cleanly.
*   **Obsidian / VS Code** = It acts as a Wiki. You can link between beans using `[[beans-3.1]]`.

### 4. The "Agent" Preference
LLMs (Claude, GPT-4) speak Markdown natively. It is their "native tongue."

*   When you feed an LLM a YAML file with a massive string block, it consumes tokens parsing the indentation structure.
*   When you feed an LLM Markdown, it understands the logical separation of Header (Metadata) vs. Body (Instructions).

### The Proposed Pivot

Change the file extension to `.md`.

**The New Schema:**
```markdown
---
# METADATA (Structured Data for the CLI)
id: beans-3.2
title: Implement bn list
status: open
priority: 2
parent: beans-3
deps:
  - beans-2
labels: [cli, rust]
assignee: @senior-dev
created: 2026-01-29T10:00:00Z
---

# Description
(The context for the agent)

# Acceptance
- [ ] Output is sorted
- [ ] JSON flag works

# Notes
(Append-only log from agents)
```

### How this changes the `bn` Binary

1.  **Parsing:** You now need a parser that splits the file at the second `---`.
    *   Top part -> YAML Parser (`serde_yaml`).
    *   Bottom part -> String (Content).
2.  **`bn show`:**
    *   Can now render the bottom part using a terminal Markdown renderer like `termimad` or `skin`.
3.  **`bn edit`:**
    *   Opens your `$EDITOR`. You get full Markdown syntax highlighting for the description.

### The "Hacker" Bonus: The Obsidian Vault

If you switch to Markdown, your `.beans/` directory becomes a valid **Obsidian Vault**.

*   You can open your project management folder in Obsidian.
*   You can use the "Graph View" in Obsidian to visualize your task dependencies (if you use Wikilinks `[[beans-2]]` in the frontmatter or body).
*   You effectively get a GUI for free.

### Conclusion

**YAML** is for configuration.
**Markdown** is for documentation and context.

Since a Bean is a **unit of context**, **Markdown** is the superior format. Keep the YAML, but constrain it to the Frontmatter.
