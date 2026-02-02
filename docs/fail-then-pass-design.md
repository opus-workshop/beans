# Fail-Then-Pass Verification

## Problem

Agents can write "cheating tests" that don't verify anything:

```python
def test_something():
    assert True  # always passes
```

## Solution

Require the verify command to FAIL before a bean can be created:

```
bn quick "fix unicode urls" --verify "pytest test_unicode.py" --fail-first
```

1. Run verify command → must FAIL (proves it tests something real)
2. Bean created
3. Agent does work
4. `bn close` → verify must PASS

## CLI Changes

```rust
// In cli.rs, Quick command:
/// Require verify to fail before bean creation (enforced TDD)
#[arg(long)]
fail_first: bool,
```

## Implementation

```rust
// In commands/quick.rs, before creating the bean:

if args.fail_first {
    let verify_cmd = args.verify.as_ref()
        .ok_or_else(|| anyhow!("--fail-first requires --verify"))?;
    
    let project_root = beans_dir.parent()
        .ok_or_else(|| anyhow!("Cannot determine project root"))?;
    
    println!("Running verify (must fail): {}", verify_cmd);
    
    let status = std::process::Command::new("sh")
        .args(["-c", verify_cmd])
        .current_dir(project_root)
        .status()?;
    
    if status.success() {
        anyhow::bail!(
            "Cannot create bean: verify command already passes!\n\
             \n\
             The test must FAIL on current code to prove it tests something real.\n\
             Either:\n\
             - The test doesn't actually test the new behavior\n\
             - The feature is already implemented\n\
             - The test is a no-op (assert True)"
        );
    }
    
    println!("✓ Verify failed as expected - test is real");
}
```

## Example Flow

```bash
# Cheating attempt - test already passes
$ bn quick "fix unicode" --verify "python -c 'assert True'" --fail-first
Running verify (must fail): python -c 'assert True'
error: Cannot create bean: verify command already passes!

The test must FAIL on current code to prove it tests something real.

# Real test - fails on current code
$ bn quick "fix unicode" --verify "pytest tests/test_unicode.py::test_fetch" --fail-first
Running verify (must fail): pytest tests/test_unicode.py::test_fetch
FAILED tests/test_unicode.py::test_fetch - URLError: ...
✓ Verify failed as expected - test is real
Created and claimed bean 5: fix unicode (by pi-agent)

# After implementing...
$ bn close 5
Running verify: pytest tests/test_unicode.py::test_fetch
PASSED tests/test_unicode.py::test_fetch
✓ Verify passed for bean 5
Closed bean 5
```

## Future Considerations

1. **Make it default?** Once proven, could make `--fail-first` the default for `bn quick` when `--verify` is provided

2. **`--skip-fail-check`** - escape hatch for special cases (build commands, existing tests)

3. **Store in bean metadata** - record that this bean used fail-first, for audit trail

4. **Integration with spro** - checkpoints already exist, could auto-verify against checkpoint
