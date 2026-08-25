// Integration tests for shipcheck's CLI entrypoint.
// Each test builds a sample repo fixture under /tmp, writes a .shipcheck.yml,
// runs the compiled binary and asserts exit codes + output format.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

/// Create a unique fixture directory under /tmp with the given config text
/// and return its path.
fn fixture_dir(name: &str, config: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("shipcheck-it-{name}-{}", nanos));
    fs::create_dir_all(&dir).expect("create fixture dir");
    fs::write(dir.join(".shipcheck.yml"), config).expect("write config");
    dir
}

fn run_binary(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_shipcheck"))
        .args(args)
        .output()
        .expect("run shipcheck binary")
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

const PASSING_CONFIG: &str =
    "checks:\n  - name: lint\n    cmd: echo linting\n  - name: fmt\n    cmd: true\n";

#[test]
fn all_checks_pass_exits_zero_with_table() {
    let dir = fixture_dir("pass", PASSING_CONFIG);
    let cfg = dir.join(".shipcheck.yml");
    let out = run_binary(&[cfg.to_str().unwrap()]);

    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    let s = stdout(&out);
    assert!(s.contains("shipcheck: running 2 checks in parallel"));
    // Both checks appear with PASS markers, in original config order.
    assert!(s.contains("PASS"), "no PASS lines: {s}");
    assert!(s.find("lint").unwrap() < s.find("fmt").unwrap());
    assert!(s.contains("✅ all checks passed — ship it"));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn failing_check_exits_one_and_reports_fail_line() {
    let dir = fixture_dir(
        "fail",
        "checks:\n  - name: ok\n    cmd: true\n  - name: broken\n    cmd: echo boom >&2; exit 3\n",
    );
    let cfg = dir.join(".shipcheck.yml");
    let out = run_binary(&[cfg.to_str().unwrap()]);

    assert_eq!(out.status.code(), Some(1), "stderr: {}", stderr(&out));
    let s = stdout(&out);
    assert!(s.contains("FAIL"));
    assert!(s.contains("broken"));
    assert!(s.contains("boom")); // last stderr line of the failing check
    assert!(s.contains("❌ 1 check(s) failed — fix before pushing"));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn missing_config_file_exits_two_with_error_on_stderr() {
    let out = run_binary(&["/nonexistent/definitely/not/here/.shipcheck.yml"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(stdout(&out).is_empty());
    assert!(stderr(&out).contains("cannot read"));
    assert!(stderr(&out).contains("/nonexistent/definitely/not/here/.shipcheck.yml"));
}

#[test]
fn empty_config_exits_two_no_checks_defined() {
    let dir = fixture_dir("empty", "");
    let cfg = dir.join(".shipcheck.yml");
    let out = run_binary(&[cfg.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(2));
    assert!(stdout(&out).is_empty());
    assert!(stderr(&out).contains("no checks defined"));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn invalid_config_yaml_exits_two_invalid_config() {
    let dir = fixture_dir("bad-yaml", "checks:\n  - name: [unclosed\n    cmd: {{{\n");
    let cfg = dir.join(".shipcheck.yml");
    let out = run_binary(&[cfg.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(2));
    assert!(stdout(&out).is_empty());
    assert!(stderr(&out).contains("invalid config"));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn json_mode_stdout_is_pure_json_array_pass_case() {
    let dir = fixture_dir("json-pass", PASSING_CONFIG);
    let cfg = dir.join(".shipcheck.yml");
    let out = run_binary(&["--json", cfg.to_str().unwrap()]);

    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    let s = stdout(&out);
    // Table output was redirected to stderr in --json mode...
    assert!(!s.contains("running"), "table leaked to stdout: {s}");
    // ...and stdout parses as a JSON array of check results.
    let parsed: serde_json::Value = match serde_json::from_str(&s) {
        Ok(v) => v,
        Err(_) => panic!("stdout is not valid JSON: {s}"),
    };
    let items = parsed.as_array().expect("JSON array");
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["name"], "lint");
    assert_eq!(items[0]["status"], "pass");
    assert!(items[0]["duration_s"].as_f64().unwrap() >= 0.0);
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn json_mode_failure_reports_fail_item_and_exit_one() {
    let dir = fixture_dir(
        "json-fail",
        "checks:\n  - name: broken\n    cmd: echo kaput >&2; exit 5\n",
    );
    let cfg = dir.join(".shipcheck.yml");
    let out = run_binary(&["--json", cfg.to_str().unwrap()]);

    assert_eq!(out.status.code(), Some(1), "stderr: {}", stderr(&out));
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout(&out)).expect("valid JSON even on failure");
    let items = parsed.as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["status"], "fail");
    assert_eq!(items[0]["exit_code"], 5);
    assert_eq!(items[0]["detail"], "kaput");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn version_flag_prints_cargo_pkg_version_and_exits_zero() {
    let out = run_binary(&["--version"]);

    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    let expected = format!("shipcheck {}\n", env!("CARGO_PKG_VERSION"));
    assert!(stderr(&out).is_empty(), "unexpected stderr");
    // stdout is exactly "shipcheck <Cargo pkg version>".
    assert_eq!(stdout(&out), expected);
}

#[test]
fn short_version_flag_also_works() {
    let out = run_binary(&["-V"]);

    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        stdout(&out),
        format!("shipcheck {}\n", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn unknown_flag_is_treated_as_no_positional_path_error_path() {
    // Unrecognized flags are not paths; binary falls back to the default
    // config path and errors with exit code 2 when it cannot be read.
    let out = Command::new(env!("CARGO_BIN_EXE_shipcheck"))
        .arg("--definitely-not-a-flag")
        .current_dir(std::env::temp_dir())
        .output()
        .expect("run shipcheck binary");
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("cannot read"));
}

#[test]
fn default_path_used_when_no_positional_arg() {
    // Run inside the fixture directory so the implicit .shipcheck.yml is found.
    let dir = fixture_dir("default-path", PASSING_CONFIG);
    let out = Command::new(env!("CARGO_BIN_EXE_shipcheck"))
        .current_dir(&dir)
        .output()
        .expect("run shipcheck binary");

    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert!(stdout(&out).contains("running 2 checks in parallel"));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn optional_check_failure_warns_but_exits_zero() {
    let dir = fixture_dir(
        "optional",
        "checks:\n  - name: wip\n    cmd: exit 9\n    optional: true\n",
    );
    let cfg = dir.join(".shipcheck.yml");
    let out = run_binary(&[cfg.to_str().unwrap()]);

    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    let s = stdout(&out);
    assert!(s.contains("WARN"));
    assert!(s.contains("exit 9"));
    assert!(s.contains("✅ all checks passed — ship it"));
    fs::remove_dir_all(&dir).ok();
}

// ---- --quiet flag ----

#[test]
fn quiet_mode_all_pass_prints_nothing_and_exits_zero() {
    let dir = fixture_dir("quiet-pass", PASSING_CONFIG);
    let cfg = dir.join(".shipcheck.yml");
    let out = run_binary(&["--quiet", cfg.to_str().unwrap()]);

    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert!(
        stdout(&out).is_empty(),
        "quiet mode should suppress success output, got: {}",
        stdout(&out)
    );
    assert!(stderr(&out).is_empty());
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn quiet_mode_failure_shows_only_fail_lines_and_summary() {
    let dir = fixture_dir(
        "quiet-fail",
        "checks:\n  - name: ok\n    cmd: true\n  - name: broken\n    cmd: echo boom >&2; exit 3\n",
    );
    let cfg = dir.join(".shipcheck.yml");
    let out = run_binary(&["--quiet", cfg.to_str().unwrap()]);

    assert_eq!(out.status.code(), Some(1), "stderr: {}", stderr(&out));
    let s = stdout(&out);
    // Errors are still visible...
    assert!(s.contains("FAIL"));
    assert!(s.contains("broken"));
    assert!(s.contains("boom"));
    assert!(s.contains("❌ 1 check(s) failed — fix before pushing"));
    // ...but everything non-error is suppressed.
    assert!(!s.contains("running"), "header leaked in quiet mode: {s}");
    assert!(!s.contains("PASS"), "PASS line leaked in quiet mode: {s}");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn quiet_mode_optional_warn_suppressed_exits_zero() {
    let dir = fixture_dir(
        "quiet-warn",
        "checks:\n  - name: wip\n    cmd: exit 9\n    optional: true\n",
    );
    let cfg = dir.join(".shipcheck.yml");
    let out = run_binary(&["--quiet", cfg.to_str().unwrap()]);

    assert_eq!(out.status.code(), Some(0));
    assert!(
        stdout(&out).is_empty(),
        "WARN should be suppressed: {}",
        stdout(&out)
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn quiet_config_errors_still_reach_stderr_with_exit_two() {
    // Quiet suppresses non-error *output*, not error reporting itself.
    let out = run_binary(&["--quiet", "/nonexistent/definitely/not/here/.shipcheck.yml"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(stdout(&out).is_empty());
    assert!(stderr(&out).contains("cannot read"));
}

#[test]
fn quiet_flag_is_not_treated_as_a_path() {
    // `shipcheck --quiet` alone must use the default config path, not
    // treat "--quiet" as a positional filename.
    let out = Command::new(env!("CARGO_BIN_EXE_shipcheck"))
        .arg("--quiet")
        .current_dir(std::env::temp_dir())
        .output()
        .expect("run shipcheck binary");
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("cannot read .shipcheck.yml"));
}

// ---- performance (benchmark-ish, not a strict gate) ----

/// 16 checks that each sleep 0.2s must finish in well under their serial
/// total (~3.2s + spawn overhead): proves checks actually run in parallel.
/// Budget of 1.6s = 8x speedup floor; CI-noise tolerant while still failing
/// loudly if execution ever regresses to serial.
#[test]
fn many_sleeping_checks_complete_in_parallel_under_wall_budget() {
    let mut cfg = String::from("checks:\n");
    for i in 0..16 {
        cfg.push_str(&format!("  - name: sleep-{i}\n    cmd: sleep 0.2\n"));
    }
    let dir = fixture_dir("perf-parallel", &cfg);
    let cfg_path = dir.join(".shipcheck.yml");
    let start = std::time::Instant::now();
    let out = run_binary(&[cfg_path.to_str().unwrap()]);
    let wall = start.elapsed();

    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert!(stdout(&out).contains("running 16 checks in parallel"));
    // Serial execution would need >= 3.2s; parallel should be ~0.25s.
    assert!(
        wall.as_secs_f64() < 1.6,
        "checks appear to run serially: wall {wall:.2?} for 16 x 0.2s sleeps"
    );
    fs::remove_dir_all(&dir).ok();
}

/// A large config (200 fast checks) exercises thread fan-out and ordering;
/// it must complete promptly and preserve config order in the output.
#[test]
fn large_config_200_checks_runs_fast_and_ordered() {
    let mut cfg = String::from("checks:\n");
    for i in 0..200 {
        cfg.push_str(&format!("  - name: chk-{i:03}\n    cmd: true\n"));
    }
    let dir = fixture_dir("perf-large", &cfg);
    let cfg_path = dir.join(".shipcheck.yml");
    let start = std::time::Instant::now();
    let out = run_binary(&["--json", cfg_path.to_str().unwrap()]);
    let wall = start.elapsed();

    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    let parsed: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid JSON");
    let items = parsed.as_array().unwrap();
    assert_eq!(items.len(), 200);
    // Order preserved despite parallel completion.
    assert_eq!(items[0]["name"], "chk-000");
    assert_eq!(items[199]["name"], "chk-199");
    // 200 process spawns on Linux take ~0.5s here; allow generous headroom.
    assert!(
        wall.as_secs_f64() < 10.0,
        "large-config run unexpectedly slow: {wall:.2?}"
    );
    fs::remove_dir_all(&dir).ok();
}
