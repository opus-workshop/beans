# Beans Best Practices for Agents

A walkthrough guide for agents (and developers) on creating, executing, and managing beans effectively.

---

## Table of Contents

1. [Beans vs Todos: When to Use Each](#beans-vs-todos-when-to-use-each)
2. [Bean Anatomy](#bean-anatomy)
3. [Creating Effective Beans](#creating-effective-beans)
4. [Writing Descriptions That Agents Can Execute](#writing-descriptions-that-agents-can-execute)
5. [Acceptance Criteria & Verification](#acceptance-criteria--verification)
6. [Hierarchical Decomposition](#hierarchical-decomposition)
7. [The Agent Workflow](#the-agent-workflow)
8. [Dependency Management](#dependency-management)
9. [Common Mistakes & How to Avoid Them](#common-mistakes--how-to-avoid-them)
10. [Practical Walkthroughs](#practical-walkthroughs)

---

## Beans vs Todos: When to Use Each

Both beans and todos track work, but they optimize for different scenarios. Choose the right tool for the task at hand.

### Use **Beans** when:

- **Multi-step work** — The task spans 3+ steps or multiple files
- **Verification matters** — You need a concrete command to prove it's done
- **Agents will execute it** — The work is handed to an LLM agent to implement autonomously
- **Dependencies exist** — Some work blocks other work
- **Context is complex** — Rich background, multiple file references, strategic decisions
- **Reusable for future sessions** — The work description needs to persist across attempts
- **Decomposition helps** — Breaking into smaller sub-beans clarifies the path forward

### Use **Todos** when:

- **Single session work** — You're doing it right now, in this conversation
- **Straightforward task** — Single simple action (add a comment, run a command, fix a typo)
- **No verification needed** — Obvious when it's done (no test, no command to run)
- **Human doing the work** — Not handing off to an autonomous agent
- **Quick coordination** — Tracking what we're doing in the next 5 steps of this session

### Decision Tree

```
Does the work span multiple steps/files?
├─ No, it's simple → USE TODOS
└─ Yes, it's multi-step
   ├─ Is an agent executing it?
   │  ├─ No, you're doing it → USE TODOS (maybe)
   │  └─ Yes, agent will execute → USE BEANS
   └─ Will it need retry attempts?
      ├─ No → USE TODOS
      └─ Yes → USE BEANS
```

---

## Bean Anatomy

Every bean has fields that serve specific purposes. Understand what goes where.

```yaml
# IDENTITY
id: '3.2.1'                                    # Auto-assigned, read-only
title: Implement token refresh logic           # One-liner summary (required)
status: open                                   # open | in_progress | closed
priority: 2                                    # 0-4 (P0 critical, P4 trivial)
created_at: 2026-01-26T15:00:00Z              # Auto-set, read-only
updated_at: 2026-01-26T15:00:00Z              # Auto-updated, read-only

# RELATIONSHIPS
parent: '3.2'                                  # Parent bean ID (if child)
dependencies:                                 # List of bean IDs this waits for
  - '2.1'
  - '2.2'
labels:                                       # Categorical tags
  - auth
  - backend

# CONTENT (Agent Prompt)
description: |                                # Rich context for the agent
  Implement token refresh logic in the auth service.

  ## Current State
  - Tokens expire after 1 hour
  - No refresh endpoint exists

  ## Task
  1. Add refresh_token grant type to src/auth/grants.rs
  2. Implement refresh endpoint in src/routes/auth.rs
  3. Add client-side retry in src/client/auth.ts

  ## Files to Modify
  - src/auth/grants.rs (add RefreshToken variant)
  - src/routes/auth.rs (POST /token endpoint)
  - src/client/auth.ts (handle 401 with refresh)

acceptance: |                                 # Concrete, testable criteria
  - Token refresh endpoint returns 200 with new access_token
  - Expired access token triggers automatic refresh
  - Refresh token expiry is enforced (7 days)
  - Existing access token still works (no immediate refresh)
  - Test: npm test -- --grep "refresh"

# VERIFICATION & EXECUTION
verify: npm test -- --grep "refresh" && npm run build
max_attempts: 3                               # Retry limit before escalation
attempts: 0                                   # Current attempt count

# EXECUTION STATE
claimed_by: "agent-7"                         # Who claimed this bean
claimed_at: 2026-01-27T10:30:00Z             # When claimed
closed_at: null                               # When closed (if closed)
close_reason: null                            # Why closed

# NOTES (Append-Only History)
notes: |
  2026-01-27 10:30 — claimed by agent-7
  2026-01-27 10:45 — attempted 1: test failures in refresh logic
  2026-01-27 11:15 — released by agent-7, see description for context
assignee: alice@example.com                   # Human owner (optional)
```

### Field Purposes at a Glance

| Field | Who sets | Why | Mutable |
|-------|----------|-----|---------|
| `id` | System | Unique identifier | No |
| `title` | Creator | Brief summary | Yes |
| `status` | System (via commands) | Workflow state | No (use `claim`, `close`) |
| `priority` | Creator | Scheduling priority | Yes |
| `description` | Creator | Agent prompt (full context) | Yes |
| `acceptance` | Creator | What "done" means | Yes |
| `verify` | Creator | Gate to closing (shell command) | Yes |
| `dependencies` | Creator | Scheduling constraints | Yes |
| `claimed_by` | Agent | Who's working on it | Auto |
| `notes` | Anyone | Execution log (timestamps auto) | Append-only |

---

## Creating Effective Beans

### Size Your Beans Right

A single bean should be **completable by one agent in one attempt** without needing to ask clarifying questions.

#### Too Big (Break into children)

```yaml
title: Build authentication system
description: |
  Implement user registration, login, token refresh, MFA, password reset...
```

This is 2-3 weeks of work. **Split it.**

#### Just Right

```yaml
title: Implement token refresh endpoint
description: |
  Add a POST /token endpoint that accepts refresh_token grant type.

  Context:
  - Access tokens expire after 1 hour
  - Refresh tokens are 30-day JWTs signed with HMAC-SHA256
  - Return { access_token, expires_in, token_type }

  Files:
  - src/routes/auth.rs

  Acceptance:
  - Endpoint validates signature and expiry
  - Returns 401 if refresh token expired
  - Returns new access_token with 1-hour expiry

  Test: npm test -- --grep "refresh"
```

This is **1-2 hours** of focused work. **Good size.**

#### Too Small (Combine or track differently)

```yaml
title: Add a comment to the refresh function
description: Explain what the signature validation does
```

Use a todo for this, not a bean.

### Estimating Bean Size

Ask yourself:
- **How many files will the agent modify?** → 2-5 ideally
- **How many functions to write/modify?** → 1-5 ideally
- **How many reads to understand context?** → 2-10 files
- **How many tests to write?** → 3-10 test cases
- **Token budget** → estimate <64k tokens total

If any of these balloons, split the bean.

### Priority Guidelines

```
P0 — Blocking multiple beans, critical path
P1 — High value, unblocks work
P2 — Standard priority (default)
P3 — Nice to have, lower urgency
P4 — Wishlist, can defer indefinitely
```

---

## Writing Descriptions That Agents Can Execute

The description is **the agent prompt**. It lives in the bean file, so agents can read it without CLI dependencies.

### Structure for Agent Success

Write descriptions that answer these questions **in order**:

1. **What's the current state?** — Context the agent needs
2. **What do you want the agent to build?** — Concrete deliverable
3. **Which files touch?** — File paths upfront
4. **How to verify?** — What "done" looks like

### Example: Good Description

```yaml
description: |
  ## Context
  The authentication service needs to refresh expired tokens.
  Currently, clients get a 401 error and must re-login.
  We want silent refresh: the client detects 401, refreshes
  the token, and retries the original request.

  ## Task
  Implement token refresh in the auth service:
  1. Add POST /api/auth/refresh endpoint
  2. Accept refresh_token in request body
  3. Validate signature and expiry
  4. Return new access_token (1-hour expiry)

  ## Important Files
  - src/routes/auth.rs — where to add the endpoint
  - src/auth/mod.rs — verify_token and issue_token functions
  - tests/auth_test.rs — where refresh tests go

  ## Edge Cases
  - Refresh token expired → return 401
  - Invalid signature → return 401
  - Valid refresh token → return 200 with new access_token

  ## How to Test
  Run: npm test -- --grep "token.*refresh"
```

### Example: Poor Description

```yaml
description: |
  Add token refresh.
```

This is **too vague**. Agent will ask clarifying questions, wasting time and tokens.

### Description Dos & Don'ts

#### Do ✅

- **Start with context** — Why are we doing this?
- **Name the files** — `src/routes/auth.rs`, not just "the routes"
- **Show the shape of the code** — "Add a `RefreshToken` variant to the `GrantType` enum"
- **List edge cases** — What should fail? What should succeed?
- **Link to examples** — "See `IssueToken` in src/auth/mod.rs for the pattern"
- **Specify the test command** — `npm test -- --grep "refresh"` not "write tests"

#### Don't ❌

- **Be vague** — "Make it work" is not enough
- **Force exploration** — Don't make the agent dig through code to understand structure
- **Mix concerns** — One bean = one feature. Don't bundle "refresh tokens" with "MFA" in one bean
- **Assume prior context** — Every description is read cold. Include what you need
- **Delegate design decisions** — Tell the agent the approach, not "figure out the best way"

---

## Acceptance Criteria & Verification

Acceptance criteria define when the work is done. Verification is the test that proves it.

### Acceptance Criteria: Human-Readable Definition

Write **concrete, testable statements**, not vague goals.

#### Too Vague ❌

```yaml
acceptance: |
  - Token refresh works correctly
  - Security is maintained
  - Performance is good
```

What does "works correctly" mean? How do we test "good performance"?

#### Concrete ✅

```yaml
acceptance: |
  - POST /api/auth/refresh accepts refresh_token in body
  - Returns 200 with { access_token, expires_in, token_type }
  - Returns 401 if refresh_token is expired or invalid
  - New access_token is valid for exactly 3600 seconds
  - Invalid signature returns 401 (no partial tokens)
  - All existing token validation tests pass
```

Each criterion should be **testable by the verify command**.

### Verify: The Machine-Checkable Gate

The `verify` field is a shell command that proves the bean is done. `bn close` runs it.

**Rules:**
- Must exit with code **0 if successful**, non-zero if failed
- Runs from the project root (wherever `.beans/` is)
- Can be a single command or shell script with `&&` chaining
- Examples:
  - `npm test -- --grep "refresh"`
  - `cargo test auth::refresh`
  - `python -m pytest tests/test_auth.py -k refresh`
  - `./scripts/verify-feature.sh`

#### Good Verify Commands

```yaml
# Single test suite
verify: npm test -- --grep "token.*refresh"

# Multiple gates
verify: npm test && npm run lint && npm run type-check

# Custom script
verify: ./scripts/verify-auth-refresh.sh

# Cargo test with specific module
verify: cargo test --lib auth::refresh -- --nocapture
```

#### Poor Verify Commands

```yaml
# Too broad (will catch unrelated failures)
verify: npm test

# Not deterministic (manual inspection)
verify: "echo 'Check if refresh works'"

# Requires interaction
verify: "read -p 'Does refresh work?' && echo ok"

# Always passes (useless)
verify: "echo 'Done!'"
```

### Linking Acceptance to Verify

The acceptance criteria **define what to test**. The verify command **proves it**.

```yaml
acceptance: |
  - POST /auth/refresh accepts { refresh_token }
  - Returns { access_token, expires_in }
  - Returns 401 if token expired
  - Returns 401 if signature invalid

verify: npm test -- --grep "refresh"
# ↑ This test suite must cover all acceptance criteria
```

**Before closing:** the agent should have written tests for every acceptance criterion. The verify command runs those tests.

---

## Hierarchical Decomposition

Strategic parents provide context. Leaf beans are agent-executable units.

### Parent Bean (Strategic Context)

Parents are **not meant to be closed**. They exist to provide context and bundle related work.

```yaml
id: '3'
title: Implement User Authentication
status: open
priority: 1
description: |
  ## Overview
  Build a complete user auth system with registration, login, and token refresh.

  ## Architecture Decision
  - Use JWT tokens (stateless)
  - Refresh tokens stored in httpOnly cookies
  - Access tokens in memory (client-side)
  - HMAC-SHA256 for signing

  ## Phased Approach
  1. Registration & login endpoints (3.1)
  2. Token refresh logic (3.2)
  3. Client-side auth manager (3.3)
  4. MFA optional features (3.4+)

  ## Files
  - Backend: src/routes/auth.rs, src/auth/mod.rs
  - Frontend: src/client/auth.ts
  - Tests: tests/auth_test.rs

  ## Common Gotchas
  - Token rotation: refresh increments version number
  - Cookie security: httpOnly, secure, sameSite=strict
  - Client retry: 401 triggers refresh, then retry original request
```

### Leaf Beans (Executable Units)

Leaf beans are children that **an agent can claim and close**.

```yaml
id: '3.1'
title: Implement user registration endpoint
parent: '3'
status: open
priority: 1
dependencies: []
description: |
  ## Context
  See parent bean 3 for architecture overview.

  ## Task
  Implement POST /api/auth/register endpoint.

  1. Accept { email, password, name }
  2. Validate email format and password strength
  3. Hash password with bcrypt (cost 12)
  4. Create user record in database
  5. Return { id, email, name, created_at }

  ## Files
  - src/routes/auth.rs — add register route
  - src/auth/mod.rs — hash_password, create_user functions
  - tests/auth_test.rs — registration tests

  ## Edge Cases
  - Email already registered → 409 Conflict
  - Password too weak → 400 Bad Request (list requirements)
  - Database error → 500 Internal Server Error

  Test: npm test -- --grep "register"

acceptance: |
  - POST /api/auth/register accepts { email, password, name }
  - Rejects duplicate emails with 409
  - Rejects weak passwords with 400 (must include requirements)
  - Creates user with hashed password (verifiable with bcrypt)
  - Returns user object with id, email, name, created_at

verify: npm test -- --grep "register" && npm run build
```

### Decomposition Rules

1. **Parent should not be closed** — It's context and organization
2. **Leaves should be closeable** — One agent, one attempt, verifiable
3. **Children inherit parent's context** — Don't repeat architecture docs
4. **Dependencies should cross hierarchy** — "3.3 depends on 3.1" means can't start until 3.1 is done

---

## The Agent Toolkit

When executing beans, agents have access to a powerful toolkit from `~/.claude/skills/agent-prompt-template.md`:

### Context Gathering (Do This First)

```bash
# Get all files referenced in a bean's description
bctx <bean-id>

# Check project specs for constraints
spec context "token refresh implementation"
```

Use `bctx` to get files referenced in bean descriptions. This solves the "cold start" problem—instead of exploring, you get all relevant files immediately.

### Safety & Rollback

```bash
# Create rollback point before risky changes
undo checkpoint "before-refresh-impl"

# Roll back if things go wrong
undo restore {checkpoint-id}
```

### Verification

```bash
# Fast check after significant edits (lint + types)
verify --quick

# Full check before committing (lint, types, build, test)
verify
```

### Error Recovery

```bash
# Check if this error has a known solution
error-db match "{error message}"

# Iterate until tests pass
loop start "{fix task}" --promise "{test cmd}"

# Record new error pattern for future agents
error-db add --pattern "{regex}" --solution "{fix}"
```

### Handoff Notes

```bash
# Write notes for downstream workers
bn update <bean-id> --note "Implemented X in file Y, tests in Z"
```

This toolkit is **standard for all agents** in the project. Agents should use these tools instead of exploring blindly.

---

## The Agent Workflow

This is how agents use beans in practice.

### Step 0: Prepare Context (New!)

Before claiming, agents gather context with the toolkit:

```bash
# Get files referenced in bean description
bctx <bean-id>

# Check project specs
spec context "user authentication"

# Create rollback point
undo checkpoint "before-registration-impl"
```

This prevents wasted time exploring blindly.

### Step 1: Agent Claims Work

Agent finds ready beans:

```bash
bn ready
```

Output:
```
P0  1.1   Implement user registration endpoint
P1  1.2   Implement login endpoint
P2  2.1   Token refresh logic
```

Agent claims a bean (atomic — only one agent can win):

```bash
bn claim 1.1
```

Status transitions: **open → in_progress**. The bean is now claimed by this agent.

**What the agent sees:**
- Read `.beans/1.1.yaml` directly (no CLI needed)
- Full description with context
- Acceptance criteria
- Verification command
- Notes from previous attempts (if retried)

### Step 2: Agent Works

Agent modifies code, writes tests, iterates locally. Uses toolkit for safety:

```bash
# Make changes
vim src/routes/auth.rs
# ... implement ...

# Quick verification (fast)
verify --quick
# If OK, continue...

# Write tests
vim tests/auth.test.ts
# ...

# Test the bean's verify command (without closing)
bn verify 1.1
```

This runs the verify command **without closing the bean**. If tests fail, agent debugs or rolls back:

```bash
# If needed, roll back to checkpoint
undo restore {checkpoint-id}
```

**Mid-work notes (recommended):**

```bash
bn update 1.1 --note "Added password validation, testing edge cases"
```

Notes are timestamped automatically. Useful for logging progress if the bean needs to be released.

### Step 3: Agent Closes (or Releases)

#### Success Path

Agent believes work is done:

```bash
# Full verification before closing (lint, types, build, test)
verify

# If all checks pass:
bn close 1.1
```

What happens:
1. Verify command runs: `npm test -- --grep "register"`
2. If exit code 0 → Bean closes. Status: **in_progress → closed**. `closed_at` set.
3. Dependents (e.g., 1.2) become ready

#### Failure Path

Verify fails (exit code non-zero):

1. All changes are undone (via `/ai-tools` or manual revert)
2. Status stays: **in_progress → open**
3. `attempts` incremented
4. Claim is released
5. Bean is available for retry

**Retry example:**

Agent 1 claimed 1.1, worked, failed verify, released.
Agent 2 runs `bn ready`, sees 1.1 again.
Agent 2 claims 1.1, reads `.beans/1.1.yaml` and notes from Agent 1.
Agent 2 gathers fresh context with toolkit (bctx, spec context).
Agent 2 knows what Agent 1 tried and avoids the same mistake.

#### Middle Path: Release Without Closing

Agent realizes the bean needs more context or design work:

```bash
bn claim 1.1 --release
```

Status: **in_progress → open**, claim released.
Agent adds notes:

```bash
bn update 1.1 --note "Need to clarify password validation rules with team"
```

Human reads the notes, updates the description, re-prioritizes.

#### Handoff & Commit

When work is verified and closed:

```bash
# Write handoff notes for downstream workers
bn update 1.1 --note "Implemented registration in src/routes/auth.rs, tests in tests/auth.test.ts, validates email uniqueness and password strength"

# Commit with bean ID prefix
git add -A
git commit -m "[beans-1.1] Implement user registration endpoint"

# Sync at session end (optional)
bn sync --flush-only
```

### Step 4: Dependents Become Ready

Once 1.1 is closed, any bean that depended on it becomes ready:

```bash
bn dep add 1.2 1.1    # "1.2 depends on 1.1"
# ...later...
bn close 1.1          # closes successfully
bn ready              # now shows 1.2
```

---

## Smart Selectors (@ Syntax)

Instead of memorizing or typing bean IDs, use smart selectors:

### Available Selectors

| Selector | Purpose | Example |
|----------|---------|---------|
| `@latest` | Most recently created bean | `bn show @latest` |
| `@blocked` | All blocked beans (waiting on dependencies) | `bn list @blocked` |
| `@ready` | All ready beans (no blockers) | `bn list @ready --tree` |
| `@parent` | Parent of the current bean | `bn close @parent` |
| `@me` | Current bean you're working on | `bn update @me --assignee alice` |

### Examples

```bash
# Show the newest bean
bn show @latest

# List blocked beans
bn list @blocked

# Display ready beans in tree format
bn list @ready --tree

# Update your current bean's assignee
bn update @me --assignee $(whoami)

# Close a bean's parent (useful in scripts)
bn close @parent
```

This eliminates the need to remember IDs and makes scripts more readable.

---

## Dependency Management

Dependencies block work. Use them to enforce ordering.

### Add Dependencies

```bash
# "Bean 2 depends on bean 1 (waits for 1 to close)"
bn dep add 2 1

# "Bean 3 depends on both 1 and 2"
bn dep add 3 1
bn dep add 3 2
```

### Understand Blocking

```bash
bn ready    # Shows only beans with no blocking dependencies
bn blocked  # Shows beans waiting on dependencies
```

### Dependency Visualization

```bash
bn dep tree 1
```

Output:
```
1
├─ 2 (depends on 1)
│  ├─ 3 (depends on 2)
│  └─ 4 (depends on 2)
└─ 5 (depends on 1)
```

### Common Patterns

#### Sequential Work (Phases)

```
Phase 1: Design
  └─ 1.1 (no dependencies)

Phase 2: Core Implementation
  ├─ 2.1 (depends on 1.1)
  └─ 2.2 (depends on 1.1)

Phase 3: Integration
  ├─ 3.1 (depends on 2.1 and 2.2)
  └─ 3.2 (depends on 2.1 and 2.2)
```

#### Parallel Work (Independent)

```
1.1 (no dependencies)
1.2 (no dependencies)
1.3 (no dependencies)
```

All three can be claimed simultaneously by different agents.

#### Diamond Pattern

```
    2.1
   /   \
  /     \
1.1     3.1
  \     /
   \   /
    2.2
```

Both 2.1 and 2.2 depend on 1.1. 3.1 depends on both 2.1 and 2.2. Can't start 3.1 until both are done.

### Cycle Detection

Avoid cycles (A depends on B, B depends on A):

```bash
bn dep cycles
```

If you see output, fix it or the system gets stuck.

---

## Common Mistakes & How to Avoid Them

### Mistake 1: Bean Too Big

**Problem:** Bean requires 20+ functions, 15+ files, days of work.

**Symptom:** Agent gets overwhelmed, verify takes forever, context balloons.

**Fix:** Split into children.

```yaml
# Before (too big)
title: Implement authentication system

# After (better)
- 1. Design token schema
- 1.1 Implement registration
- 1.2 Implement login
- 1.3 Implement token refresh
```

### Mistake 2: Vague Description

**Problem:** "Add auth validation" without context on where, what validation, what files.

**Symptom:** Agent asks clarifying questions, wastes tokens exploring.

**Fix:** Write rich descriptions with file paths, edge cases, and examples.

```yaml
# Before
description: Validate authentication tokens

# After
description: |
  Add token validation to middleware in src/middleware/auth.ts.

  Validate: JWT signature, expiry, issuer claim.

  On invalid token: return 401 with { error: "Unauthorized" }
  On expired token: return 401 with { error: "Token expired" }
  On valid token: attach user_id to request.user

  Files: src/middleware/auth.ts, tests/middleware_test.ts
  See: src/auth/verify_token function (reuse this)
```

### Mistake 3: Unclear Acceptance Criteria

**Problem:** Criteria don't match what the agent actually tests.

**Symptom:** Agent finishes, runs verify, it fails. "But I thought it was done."

**Fix:** Make criteria testable and specific.

```yaml
# Before
acceptance: |
  - Token validation works
  - Security is maintained

# After
acceptance: |
  - Valid JWT passes validation, attaches user_id to request
  - Expired JWT returns 401
  - Invalid signature returns 401
  - Missing Authorization header returns 401
  - Malformed JWT returns 400
  All tested by: npm test -- --grep "auth.*validation"
```

### Mistake 4: Verify Command Doesn't Match Acceptance

**Problem:** You write acceptance criteria but the verify command doesn't test them.

**Symptom:** Bean closes but acceptance criteria aren't actually met.

**Fix:** Ensure every acceptance criterion has a test, and verify runs those tests.

```yaml
acceptance: |
  - Registration rejects duplicate emails with 409
  - Registration hashes passwords (never stores plaintext)
  - Registration returns { id, email, name, created_at }

verify: npm test -- --grep "register"
# ↑ This test file MUST have tests for:
#   - duplicate email rejection
#   - password hashing
#   - response shape
```

### Mistake 5: Circular Dependencies

**Problem:** A depends on B, B depends on A.

**Symptom:** `bn ready` returns nothing. No progress possible.

**Fix:** Use `bn dep cycles` and break the cycle.

```bash
# Detect cycles
bn dep cycles

# Output: "Cycle detected: 1 → 2 → 1"

# Break it
bn dep remove 2 1  # or restructure dependencies
```

### Mistake 6: Dependencies as Parent-Child Substitute

**Problem:** Using dependencies instead of hierarchy.

```yaml
# Anti-pattern
id: 1
title: Feature A

id: 2
title: Feature A - Part 2
dependencies: [1]  # Should be parent-child instead
```

**Fix:** Use hierarchy (parent.id) for decomposition, dependencies for blocking.

```yaml
# Better
id: 1
title: Feature A (parent)

id: 1.1
title: Feature A - Part 1
parent: 1

id: 1.2
title: Feature A - Part 2
parent: 1
dependencies: [1.1]  # Only if 1.2 truly requires 1.1 to be done first
```

### Mistake 7: Forgetting Context When Updating

**Problem:** Agent releases a bean with notes but no description update. Next agent is confused.

**Symptom:** Second attempt: "Wait, what's the issue? The description doesn't explain what failed."

**Fix:** When releasing, update the description with findings.

```bash
bn claim 1.1
# ... agent works, fails verify, releases ...
bn update 1.1 --note "Signature validation was failing; see line 45 of auth.rs"
# Better: edit the description to clarify the issue
```

### Mistake 8: Verify Command Too Slow or Flaky

**Problem:** Verify command takes 10+ minutes or randomly fails.

**Symptom:** Beans fail to close even when work is done.

**Fix:** Use targeted test suites, avoid full test runs.

```yaml
# Too slow
verify: npm test   # runs all 500 tests

# Better
verify: npm test -- --grep "register"  # runs 5 relevant tests
```

---

## Practical Walkthroughs

Real examples of creating and executing beans.

### Walkthrough 1: User Registration (Single Leaf Bean)

#### Scenario
You want an agent to implement a user registration endpoint. Simple feature, no blockers, self-contained.

#### Step 1: Create the Bean

```bash
bn create "Implement user registration endpoint"
```

Outputs: `New bean: 1`

#### Step 2: View It

```bash
bn show 1
```

```yaml
id: '1'
title: Implement user registration endpoint
status: open
priority: 2
created_at: 2026-01-27T10:00:00Z
description: null
acceptance: null
verify: null
```

Empty. Let's fill it in.

#### Step 3: Edit the Bean

Open `.beans/1.yaml` in your editor and fill in:

```yaml
id: '1'
title: Implement user registration endpoint
status: open
priority: 1  # User auth is high-priority
description: |
  ## Context
  The API needs a user registration endpoint.
  New users provide email, password, and name.
  Passwords are hashed with bcrypt before storage.

  ## Task
  Implement POST /api/auth/register:
  1. Accept { email, password, name } in body
  2. Validate email is unique (409 if duplicate)
  3. Validate password is strong (8+ chars, uppercase, number)
  4. Hash password with bcrypt cost 12
  5. Create user record
  6. Return { id, email, name, created_at }

  ## Files
  - src/routes/auth.rs — add /register route
  - src/auth/mod.rs — hash_password, create_user helpers
  - tests/auth_test.rs — registration tests

  ## Test It
  Run: npm test -- --grep "register"

  ## Reference
  Look at login endpoint (src/routes/auth.rs) for patterns.

acceptance: |
  - POST /api/auth/register accepts { email, password, name }
  - Rejects duplicate email with 409
  - Rejects weak password (< 8 chars) with 400
  - Hashes password before storage (never stored plaintext)
  - Returns user: { id, email, name, created_at }
  - Returns 500 on database error

verify: npm test -- --grep "register" && npm run build
```

#### Step 4: Agent Claims the Bean

```bash
bn claim 1
```

Agent reads `.beans/1.yaml` and starts work.

#### Step 5: Agent Checks Progress

Mid-work, agent runs:

```bash
bn verify 1
```

If tests pass, agent continues. If they fail, agent fixes.

#### Step 6: Agent Closes

When done:

```bash
bn close 1
```

Verify runs. If it passes, bean closes. Dependents become ready.

---

### Walkthrough 2: Complex Feature with Decomposition (Parent + Children)

#### Scenario
You want to build "Complete Authentication System."
This is too big for one bean, so break it into phases.

#### Step 1: Create Parent Bean

```bash
bn create "Complete Authentication System"
```

Output: `New bean: 2`

Edit `.beans/2.yaml`:

```yaml
id: '2'
title: Complete Authentication System
status: open
priority: 0  # Critical path
description: |
  ## Overview
  Build a complete auth system: registration, login, token refresh, MFA prep.

  ## Architecture
  - JWT tokens (HS256)
  - Refresh tokens in httpOnly cookies
  - Access tokens in memory (frontend)
  - Password: bcrypt (cost 12)

  ## Implementation Plan
  1. Phase 1: Registration & Login (2.1)
  2. Phase 2: Token Refresh (2.2)
  3. Phase 3: Client-side Auth Manager (2.3)
  4. Phase 4: Email Verification (2.4, optional)

  ## Key Files
  - Backend: src/routes/auth.rs, src/auth/mod.rs
  - Frontend: src/client/auth.ts
  - Tests: tests/auth_test.rs, src/client/__tests__/auth.test.ts

  ## Security Considerations
  - Never log passwords or tokens
  - Tokens in secure, httpOnly cookies
  - CSRF protection on token endpoints
  - Rate limit login attempts

# Leave description empty for parent; it's context only
# Don't set verify; parent beans aren't closed
```

#### Step 2: Create Phase 1 (Registration & Login)

```bash
bn create "Implement registration endpoint" --parent 2
```

Output: `New bean: 2.1`

```bash
bn create "Implement login endpoint" --parent 2
```

Output: `New bean: 2.2`

```bash
# Make 2.2 depend on 2.1 (login needs user table from registration)
bn dep add 2.2 2.1
```

Edit `.beans/2.1.yaml` and `.beans/2.2.yaml` with full descriptions (like Walkthrough 1).

#### Step 3: Create Phase 2 (Token Refresh)

```bash
bn create "Implement token refresh endpoint" --parent 2
```

Output: `New bean: 2.3`

```bash
bn dep add 2.3 2.1  # Needs registration (user table)
bn dep add 2.3 2.2  # Needs login (to understand token flow)
```

#### Step 4: View the Hierarchy

```bash
bn tree 2
```

Output:
```
[  ] 2. Complete Authentication System
  [ ] 2.1 Implement registration endpoint
  [ ] 2.2 Implement login endpoint
    └─ depends on 2.1
  [ ] 2.3 Implement token refresh endpoint
    ├─ depends on 2.1
    └─ depends on 2.2
```

#### Step 5: Check Readiness

```bash
bn ready
```

Output:
```
P0  2.1   Implement registration endpoint
```

Only 2.1 is ready (no blockers). 2.2 and 2.3 are blocked.

#### Step 6: Agents Swarm

Agent 1 claims 2.1, implements registration.
After 2.1 closes, 2.2 becomes ready.
Agent 2 claims 2.2, implements login.
After 2.2 closes, 2.3 becomes ready.
Agent 3 claims 2.3, implements token refresh.

All work in dependency order, no parallelism bottleneck.

---

### Walkthrough 3: Handling Retry (Agent Fails, Second Agent Tries)

#### Scenario
Agent 1 claims bean, works, fails verify. Agent 2 retries.

#### Step 1: Agent 1 Claims

```bash
bn claim 2.1
```

Status: open → in_progress

#### Step 2: Agent 1 Works

Agent modifies code, writes tests. Mid-work:

```bash
bn verify 2.1
# Test fails: registration test times out
```

Agent debugs, realizes there's an issue but decides to release for human review.

```bash
bn claim 2.1 --release
bn update 2.1 --note "Database timeout on create_user; check connection pool"
```

Status: in_progress → open

#### Step 3: Human Reviews

Human reads notes, sees the issue. Updates description to clarify:

```yaml
description: |
  ... existing description ...

  ## Known Issues (from previous attempt)
  - Database connection pool may be maxed; verify pool size in .env
  - See src/config/database.rs for pool config
```

#### Step 4: Agent 2 Retries

```bash
bn claim 2.1
```

Agent reads `.beans/2.1.yaml`, sees notes and updated description.
Agent checks connection pool, finds and fixes the issue.

```bash
bn close 2.1
```

Verify passes. Bean closes.

---

## The Development Loop

All work in the project follows a standard workflow:

1. **Understand** — `bctx <bean-id>` + `spec context` before touching code
2. **Plan** — Single task: just do it. Multi-step: break into beans with `bn create`
3. **Implement** — Single bean: implement directly. Epic: `/swarm` for parallel agents
4. **Verify** — `verify` before committing (lint, types, build, test)
5. **Close** — `bn close <id>` when verified, `bn sync --flush-only` at session end

This ensures consistency across all bean execution and agent work.

---

## The Beanstalk Vision: Future Toolchain

Beans is evolving from a task tracker into a comprehensive orchestration platform. Here's the strategic vision:

### Planned Companion Tools

**`bctx`** — Context Assembler (Killer App for Agents)
- Reads a bean, extracts file paths from description, concatenates file contents
- Solves the "cold start" problem for agents
- Usage: `bctx beans-3.2 | llm "Implement this"`
- Status: Partially implemented (1.1, 1.2)

**`bpick`** — Fuzzy Selector
- Interactive bean selection using `fzf`
- Never type an ID manually
- Usage: `bn close $(bpick)`
- Status: Planned

**`bmake`** — Dependency-Aware Execution
- Execute commands only when DAG permits (CI/CD gatekeeper)
- Usage: `bmake beans-50 "./deploy.sh"` (only runs if bean is closed)
- Status: Planned

**`btime`** — Punch Clock
- Calculate cycle time from `created_at` to `closed_at`
- Track active time via git log analysis
- Status: Planned

**`bgrep`** — Semantic Grep
- Search beans with field filtering
- Usage: `bgrep "database" --field description --status open`
- Status: Planned

**`bviz`** — TUI Dashboard
- Left pane: Tree view of beans
- Right pane: Markdown renderer
- Bottom pane: Dependency graph
- Status: Planned

### Infrastructure Improvements

**Git Hook Integration**
- Auto-prepend bean ID to commits
- Branch: `feat/beans-3.2-list-command` → Commit: `[beans-3.2] Added sorting`
- Status: Planned

**Markdown Format Migration**
- Current: Pure YAML files (flexible, direct file access)
- Future: Markdown with YAML frontmatter (better for agents, LLMs, humans)
- Status: Foundation in place, migration planned

**Bean Server Protocol**
- JSON-RPC interface for IDE/Agent integration
- Status: Planned

### Current Implementation Status

Already available:
- `bn claim` / `bn verify` — Atomic task claiming and verification without closing
- `bn edit` — Edit beans in $EDITOR with schema validation
- Smart selectors (@latest, @blocked, @parent, @me)
- Hook system — Pre-close hooks for CI gatekeeper patterns
- Archive system — Auto-archiving closed beans to dated directories
- Multi-format support — YAML and Markdown
- Agent toolkit — bctx, undo checkpoints, error-db, loop integration

---

## Summary

### Key Takeaways

1. **Beans for multi-step work that agents execute.** Todos for quick in-session tasks.
2. **Size beans to ~1-5 files, 1-5 functions.** Bigger = split into children.
3. **Write descriptions for cold reads.** Assume the agent doesn't have context.
4. **Acceptance criteria must be testable.** Verify command proves it.
5. **Use hierarchy for decomposition** (parent/child), **dependencies for blocking** (A waits for B).
6. **Parents provide context.** Leaves are executable.
7. **Agents use the toolkit** (bctx, undo, verify, error-db) instead of exploring blindly.
8. **Agents claim atomically.** Only one agent per bean.
9. **Verify gates closing.** No force-close. If verify fails, retry with a fresh agent.
10. **Notes are the execution log.** Timestamp automatically, visible to next agent.
11. **Dependencies enable waves.** Agents work in parallel, constrained by scheduling.
12. **Follow the development loop** — Understand → Plan → Implement → Verify → Close.

### Quick Checklist: Is My Bean Ready for Agents?

#### Before Creation
- [ ] **Standalone or part of epic?** If epic: create parent, assign as child
- [ ] **Size right?** 1-5 files, 1-5 functions modified, <64k tokens
- [ ] **Blockers identified?** Know what must be done first

#### During Creation
- [ ] **Title** — One-liner, clear and descriptive
- [ ] **Priority** — Set appropriately (P0-P4)
- [ ] **Description** — Rich context, file paths, edge cases, no exploration needed
- [ ] **Acceptance** — Concrete, testable criteria (not vague goals)
- [ ] **Verify** — Shell command that proves acceptance is met
- [ ] **Dependencies** — Only include true blocking relationships
- [ ] **Labels** — Tagged for organization (optional but helpful)

#### Before Agent Execution
- [ ] **Bean is in ready state** — `bn ready` shows it
- [ ] **Agent has toolkit installed** — bctx, undo, verify, error-db
- [ ] **Acceptance criteria are complete** — No ambiguity
- [ ] **Verify command is tested** — You've run it locally

If all checked, the bean is ready for an agent to claim.

---

## Further Reading

**Project Documentation:**
- [beans README.md](./README.md) — System overview and commands
- [Bean YAML Format](./README.md#a-bean-looks-like-this) — Field reference
- [Agent Workflow](./README.md#agent-workflow) — How agents execute beans
- [TODO.md](./TODO.md) — Strategic vision and roadmap

**Agent Resources:**
- [Agent Toolkit Instructions](../.claude/skills/agent-prompt-template.md) — Toolkit for spawned agents (bctx, undo, verify, error-db)
- Agent prompt template includes: context gathering, safety, verification, error recovery, handoff

**Command Reference:**
- `bn --help` — Full CLI help
- `bn <command> --help` — Help for specific command

**Key New Commands:**
- `bn claim <id>` — Atomically claim bean for work
- `bn claim <id> --release` — Release claim
- `bn verify <id>` — Test verify without closing
- `bn edit <id>` — Edit in $EDITOR with validation
- Smart selectors: `@latest`, `@blocked`, `@ready`, `@parent`, `@me`

---

*Last updated: 2026-01-30*
*Reflects project changes through commit 606509b*
