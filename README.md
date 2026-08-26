# shipcheck

A pre-push quality gate as a single Rust binary. Point it at a `.shipcheck.yml`, it runs every check, prints a PASS/WARN/FAIL table with per-check durations, and exits non-zero when a required check fails — so `git push` only happens after your gate is green.

No daemon, no runtime deps, no config language to learn. If you can write a shell command, you can write a shipcheck config.

## Why

Most broken pushes fail for boring reasons: lint didn't run, tests weren't rebuilt, docs drifted. CI catches these *after* the push; shipcheck catches them *before*, in about a millisecond of startup time, on your machine, with your config.

## Install

```bash
cargo install --path .
# or build and use the binary directly
cargo build --release
```

## Usage

```bash
cd your-project
$EDITOR .shipcheck.yml   # define your checks
shipcheck                # run the gate
shipcheck path/to.yml    # or point at any config
```

Typical wiring as an actual pre-push hook:

```bash
# .git/hooks/pre-push
#!/bin/sh
exec shipcheck
```

## Config

`.shipcheck.yml` — either a bare list of checks or a `{checks: [...]}` document:

```yaml
checks:
  - name: lint
    cmd: ruff check .
  - name: tests
    cmd: cargo test --quiet
  - name: docs build
    cmd: mdbook build docs
    optional: true     # failure → WARN, doesn't block the push
```

Rules:

- Each `cmd` runs via `sh -c`, so anything your shell can do works (pipes, env vars, `&&` chains).
- Required check fails → `FAIL`, shipcheck exits `1`.
- `optional: true` check fails → `WARN`, exit code unaffected.
- Missing file, invalid YAML, or zero checks → exit `2` (config error, not a check failure).

## Flags

| Flag | Effect |
|---|---|
| *(none)* | Runs `.shipcheck.yml`, prints the PASS/WARN/FAIL table to stdout |
| `<path>` | First non-flag argument; config file to run (default `.shipcheck.yml`) |
| `--json` | Stdout carries **only** the JSON report; all human output moves to stderr |
| `--quiet` | Suppresses header, PASS/WARN lines, and the success summary. FAIL lines and the failure summary still print |
| `--version`, `-V` | Print version and exit |

Flags may appear anywhere; in `--json --quiet` mode stdout is pure JSON with nothing else.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | All required checks passed (warnings allowed) |
| `1` | One or more required checks failed |
| `2` | Config error: file unreadable, invalid YAML, or zero checks defined |

## JSON output (`--json`)

Stdout is a pretty-printed JSON array of check results, in config order
(regardless of completion order):

```json
[
  {
    "name": "lint",
    "status": "pass",
    "exit_code": null,
    "duration_s": 0.41231
  },
  {
    "name": "docs build",
    "status": "warn",
    "exit_code": 101,
    "duration_s": 2.0345
  },
  {
    "name": "tests",
    "status": "fail",
    "exit_code": 1,
    "detail": "test result: FAILED. 1 passed; 1 failed",
    "duration_s": 1.8723
  }
]
```

Schema per element:

- `name` (string) — check name from the config.
- `status` (string) — `"pass"`, `"warn"` (optional check failed), or `"fail"`.
- `exit_code` (number \| null) — process exit code; `null` when unknown (pass, or spawn error).
- `detail` (string, optional) — last line of stderr; present only on `"fail"`.
- `duration_s` (number) — wall time of the check in seconds.

Serialization is typed (`serde_json`), so names/details containing quotes or
backslashes are escaped correctly.

## Example output

```
shipcheck: running 3 checks

  PASS  lint                     (412.31ms)
  PASS  tests                    (1.87s)
  WARN  docs build               (2.03s) exit 101
```

## Status

v0.2 — working and verified: parallel execution, PASS/WARN/FAIL semantics, per-check durations, `--json` and `--quiet` flags, correct exit codes (1 = check failure, 2 = config error), both config shapes.

## Roadmap

- [x] Parallel execution of independent checks
- [ ] Secret-scanning pass (entropy + regex over the diff)
- [x] `--json` output for CI integration
- [ ] GitHub Action wrapper

MIT

## License

MIT — see [LICENSE](LICENSE).
