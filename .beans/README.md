# .beans/ Directory

This directory contains the beans CLI project's own task tracking. **This is not test data** — this is the project dogfooding its own tool.

## Why Dogfooding?

The beans project uses beans to track its own development. This provides:

- **Proof of concept** — we validate the tool works for real development
- **Continuous feedback** — building features we actually use immediately
- **Authentic requirements** — no hypothetical tasks, only real work
- **Living documentation** — see patterns and workflows in action

## Structure

```
.beans/
  bean.yaml       # Root goal — the project's strategic vision
  index.yaml      # Auto-rebuilt index cache (never edit manually)
  config.yaml     # Project configuration
  1.yaml          # Bean 1: Feature or task
  2.yaml          # Bean 2: Feature or task
  3.yaml          # Bean 3: Feature or task
  # ... and so on
```

Each numbered file is a **bean** — a unit of work with a title, description, acceptance criteria, and verification command.

## Reading a Bean

To see what work is in progress or planned:

```bash
bn list              # Show all beans
bn list --tree       # Show hierarchical view
bn ready             # Show beans ready to work on (no blocking dependencies)
bn show 1            # Show detailed view of bean 1
```

Or read the files directly:

```bash
cat .beans/1.yaml    # Raw YAML
less .beans/index.yaml  # Auto-built index of all beans
```

## Dependency Graph

Beans have dependencies — some work blocks other work. View the dependency structure:

```bash
bn dep tree          # Show dependency tree
bn graph --format mermaid  # Export as Mermaid diagram
```

## Status Indicators

Each bean has a status:

- `open` — ready to start or waiting on blockers
- `in_progress` — someone is actively working on it
- `closed` — completed and verified
- `cancelled` — no longer needed

## Verification

Every bean has a `verify` command that proves the work is done:

```bash
bn close 1           # Run bean 1's verify command
                     # If it exits 0, bean closes
                     # If it fails, changes undo and bean stays open
```

## Contributing

When working on this project:

1. Check `bn ready` to see work with no blockers
2. Create a new bean with `bn create "your feature"`
3. Decompose complex work into child beans
4. Add dependencies with `bn dep add <id> <blocker>`
5. Work on your bean, committing code changes normally
6. When done, `bn close <id>` to verify and close

All bean changes automatically get tracked in git at `.beans/`.

## Further Reading

See the main [README.md](../README.md) for full documentation on the beans CLI itself.
