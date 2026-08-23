# shipcheck

Pre-push quality gate as a single Rust binary. Reads `.shipcheck.yml`, runs each check via `sh -c`, prints a PASS/WARN/FAIL table with durations, exits non-zero if any required check fails.

## Usage

```bash
cargo install --path .   # or use target/release/shipcheck
cd your-project
$EDITOR .shipcheck.yml
shipcheck
```

## Config

```yaml
checks:
  - name: lint
    cmd: ruff check .
  - name: tests
    cmd: cargo test --quiet
  - name: docs build (non-blocking)
    cmd: mdbook build docs
    optional: true     # failure → WARN, doesn't block
```

## Roadmap (v0.2+)

- [ ] Parallel execution of independent checks (threads)
- [ ] Secret-scanning pass (entropy + regex over diff)
- [ ] `--json` output for CI integration
- [ ] GitHub Action wrapper
