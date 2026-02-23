# Design: Pi-Native Bean Run Tool

## Status: Draft

## Problem

Current `deli_run` is a **three-layer sandwich**:
```
pi (orchestrator agent)
  → deli_run (pi extension tool)
    → bn run --json-stream (Rust CLI)
      → pi --mode json -p (child agents, one per bean)
```

The middle layer (`bn run`) duplicates orchestration that could live in the pi extension itself. It computes waves, manages timeouts, idle detection, and streams JSON events — all things the extension could do directly. The extension currently just forwards JSON events and renders a TUI widget.

## Goal

Eliminate `bn run` from the spawning path. The pi extension reads beans directly, computes execution order, and spawns `pi` subprocesses — just like the `subagent` extension already does.

Still use `bn` CLI for **state mutations** (`claim`, `close`, `verify`, `update`) since those handle file locking, index updates, and validation.

```
pi (orchestrator agent)
  → bean_run (pi extension tool)
    → reads .beans/index.yaml + .beans/*.md
    → computes dependency waves
    → spawns pi --mode json -p (child agents)
    → calls bn claim/close for state transitions
```

## Design

### Tool: `bean_run`

Replaces `deli_run`, `deli_spawn`, `deli_pull`. Single tool, multiple modes.

```typescript
bean_run({
  // What to run
  target: "84",           // Bean ID — run this bean or its children
  
  // Execution options
  parallel: 4,            // Max concurrent agents
  dryRun: false,          // Preview waves without executing
  keepGoing: false,       // Continue past failures
  
  // Per-agent options
  timeout: 30,            // Minutes per agent
  idleTimeout: 5,         // Minutes of no output → kill
  model: "claude-sonnet-4-5",  // Override model for all spawned agents
  
  // Prompt customization
  systemPrompt: "...",    // Append to system prompt for all agents
  instructions: "...",    // Prepend to each agent's task
})
```

**Mode inference:**
- If target has open children → run children (multi-agent orchestration)
- If target is a leaf with verify → run single agent on that bean
- If target has no verify → error ("bean has no verify gate, break it down first")

### Bean Reading (TypeScript, in-process)

Parse directly instead of shelling out to `bn`:

```typescript
interface BeanIndex {
  beans: BeanEntry[];
}

interface BeanEntry {
  id: string;
  title: string;
  status: "open" | "in_progress" | "closed";
  priority: number;
  parent?: string;
  dependencies?: string[];
  produces?: string[];
  requires?: string[];
  has_verify: boolean;
  tokens?: number;
}

// Parse index.yaml
function readIndex(beansDir: string): BeanIndex

// Parse full bean file (YAML frontmatter + markdown body)
function readBean(beansDir: string, id: string): { meta: BeanEntry; body: string; path: string }

// Get children of a parent (from index, fast)
function getChildren(index: BeanIndex, parentId: string): BeanEntry[]

// Get open children ready for work
function getReadyChildren(index: BeanIndex, parentId: string): BeanEntry[]
```

### Wave Computation (TypeScript, in-process)

Topological sort on `produces`/`requires` + `dependencies`:

```typescript
interface Wave {
  round: number;
  beans: BeanEntry[];
}

function computeWaves(beans: BeanEntry[]): Wave[] {
  // 1. Build adjacency: bean B depends on bean A if:
  //    - B.requires includes something A.produces
  //    - B.dependencies includes A.id
  // 2. Topological sort into waves
  //    Wave 1: beans with no dependencies
  //    Wave N: beans whose deps are all in waves < N
  // 3. Within each wave, sort by priority (lower = higher priority)
}
```

This is ~50 lines. The logic is simple — `bn run` computes the same thing.

### Agent Spawning (direct pi subprocess)

Follow the `subagent` pattern exactly:

```typescript
async function spawnBeanAgent(
  bean: { meta: BeanEntry; body: string; path: string },
  options: { model?: string; timeout?: number; idleTimeout?: number; systemPrompt?: string },
  signal: AbortSignal,
  onEvent: (event: JsonModeEvent) => void,
): Promise<AgentResult> {
  
  // 1. Claim the bean
  await exec("bn", ["claim", bean.meta.id, "--by", "pi-agent"]);
  
  // 2. Build the prompt
  const prompt = buildAgentPrompt(bean, options);
  
  // 3. Spawn pi --mode json -p
  const args = ["--mode", "json", "-p", "--no-session"];
  if (options.model) args.push("--model", options.model);
  args.push("--append-system-prompt", writeTempFile(prompt.systemPrompt));
  args.push(prompt.userMessage);
  
  // 4. Stream JSON events, parse them
  const proc = spawn("pi", args, { cwd: process.cwd() });
  // ... same JSON streaming as subagent extension ...
  
  // 5. On completion, call bn close (or update on failure)
  if (result.success) {
    await exec("bn", ["close", bean.meta.id]);
  } else {
    await exec("bn", ["update", bean.meta.id, "--note", `Agent failed: ${result.error}`]);
    await exec("bn", ["claim", bean.meta.id, "--release"]);
  }
}
```

### Prompt Construction

This is the most important part. Currently `bn run` uses a simple template:
```
pi @$(ls .beans/{id}-*.md) "implement this bean and run bn close {id} when done"
```

Going native lets us build **much richer prompts**:

```typescript
function buildAgentPrompt(
  bean: { meta: BeanEntry; body: string },
  options: { instructions?: string; systemPrompt?: string },
): { systemPrompt: string; userMessage: string } {
  
  // System prompt additions
  const systemParts: string[] = [];
  
  // Project rules (if .beans/RULES.md exists)
  if (fs.existsSync(".beans/RULES.md")) {
    systemParts.push(fs.readFileSync(".beans/RULES.md", "utf-8"));
  }
  
  // Bean-specific context
  systemParts.push(`
You are implementing bean ${bean.meta.id}: ${bean.meta.title}

When you have completed the implementation and all acceptance criteria are met,
run: bn close ${bean.meta.id}

The close command will run the verify gate automatically. If it fails, 
fix the issue and try again.

If you get stuck or the task is unclear, run:
bn update ${bean.meta.id} --note "Stuck: <explanation>"
`);
  
  if (options.systemPrompt) {
    systemParts.push(options.systemPrompt);
  }
  
  // User message = the bean body (markdown description)
  let userMessage = bean.body;
  if (options.instructions) {
    userMessage = options.instructions + "\n\n" + userMessage;
  }
  
  return {
    systemPrompt: systemParts.join("\n\n---\n\n"),
    userMessage,
  };
}
```

### Execution Flow

```
bean_run({ target: "84", parallel: 4 })
│
├── Read index.yaml
├── Get open children of 84
├── Compute waves from produces/requires
│
├── Wave 1: [84.1, 84.2, 84.3, ...]  (no deps, run in parallel)
│   ├── Spawn pi for 84.1 (up to 4 concurrent)
│   ├── Spawn pi for 84.2
│   ├── Spawn pi for 84.3
│   ├── ... wait for all to complete ...
│   └── Refresh index (some beans may now be closed)
│
├── Wave 2: [84.14, ...]  (deps satisfied)
│   ├── Check: are requires from wave 1 actually produces'd by closed beans?
│   ├── Spawn remaining
│   └── ... wait ...
│
└── Return summary: {done: N, failed: M, waves: [...]}
```

**Improvement over `bn run`:** Fine-grained dispatch. Instead of strict wave boundaries, use a ready-queue:

```typescript
async function executeReadyQueue(
  allBeans: BeanEntry[],
  options: RunOptions,
  signal: AbortSignal,
  onUpdate: UpdateCallback,
): Promise<RunResult> {
  const completed = new Set<string>();  // IDs of closed beans
  const completedProduces = new Set<string>();  // All produced artifacts
  const running = new Map<string, Promise<AgentResult>>();
  const remaining = new Set(allBeans.map(b => b.id));
  
  while (remaining.size > 0 || running.size > 0) {
    // Find beans whose deps are all satisfied
    const ready = findReady(allBeans, remaining, completed, completedProduces);
    
    // Fill up to parallel limit
    while (ready.length > 0 && running.size < options.parallel) {
      const bean = ready.shift()!;
      remaining.delete(bean.id);
      running.set(bean.id, spawnBeanAgent(bean, ...));
    }
    
    // Wait for any one to finish
    const finished = await Promise.race(running.values());
    running.delete(finished.id);
    
    if (finished.success) {
      completed.add(finished.id);
      for (const p of finished.produces || []) completedProduces.add(p);
    }
    
    // Loop: will now find newly-unblocked beans
  }
}
```

This means bean C (depends only on A) starts as soon as A finishes, even if B is still running. No wave boundaries.

### TUI Integration

Reuse the existing `BeansProgressComponent` — it already works great. The only change is the data source: instead of parsing `bn --json-stream` events, we generate them directly from subprocess monitoring.

### What `bn` Still Handles

| Operation | Via |
|-----------|-----|
| `bn claim <id>` | State: open → in_progress |
| `bn close <id>` | Verify + state: in_progress → closed |
| `bn verify <id>` | Run verify gate without closing |
| `bn update <id> --note "..."` | Log progress/failure |
| `bn claim <id> --release` | Release failed claim |
| `bn show <id>` | Read bean details (we read directly, but this is a fallback) |

These are simple, fast commands. No orchestration logic — just state transitions with validation.

### What We No Longer Need From `bn`

| Removed from path | Why |
|--------------------|-----|
| `bn run` | Wave computation + spawning now in extension |
| `bn run --json-stream` | No longer consumed |
| `bn ready --parent` | We compute readiness from index.yaml directly |
| `bn agents` | We track agents in-memory |
| `bn logs` | We capture logs from pi subprocess directly |

### Advantages Over Current Approach

1. **Fine-grained dispatch** — ready-queue instead of strict waves
2. **Richer prompts** — custom system prompts, RULES.md injection, per-bean model selection
3. **Better error context** — we have the full pi JSON stream, can show tool calls/thinking in TUI
4. **No intermediate process** — one fewer spawn, slightly faster
5. **Customizable** — extension users can hook into events, modify prompts, add pre/post hooks
6. **Model flexibility** — different beans can use different models (haiku for small tasks, sonnet for complex ones)
7. **Subagent integration** — could use the existing subagent infrastructure instead of raw pi spawning

### Risks

1. **Logic drift** — wave computation must match `bn`'s expectations for produces/requires
2. **Index race conditions** — multiple agents closing beans simultaneously update index.yaml (mitigated by `bn close` handling locking)
3. **Token counting** — `bn` counts tokens on bean files; we'd need to respect `max_tokens` config
4. **Feature parity** — `bn run` has `--loop-mode`, `--auto-plan` etc. that we'd need to reimplement

### Implementation Plan

**Phase 1: Core** (~300 LOC)
- Bean/index YAML parser
- Wave/ready-queue computation
- Single `bean_run` tool with basic spawning

**Phase 2: TUI** (~100 LOC)
- Adapt existing BeansProgressComponent
- Stream pi JSON events into the progress widget

**Phase 3: Polish** (~200 LOC)
- Custom renderers (renderCall/renderResult)
- Model override per bean
- RULES.md injection
- Error recovery + retry logic

### File Structure

```
~/.pi/agent/extensions/beans/
├── index.ts          # Extension entry: registers bean_run tool + commands
├── parser.ts         # Read index.yaml and bean .md files
├── scheduler.ts      # Ready-queue computation, dependency graph
├── spawner.ts        # Spawn pi subprocess, parse JSON events
├── prompt.ts         # Build agent prompts from bean content
├── progress.ts       # TUI progress widget (adapted from current)
└── README.md
```
