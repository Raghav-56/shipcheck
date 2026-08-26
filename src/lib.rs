// shipcheck v0.2 library — core logic split out of main so it is testable.
//! Core logic for the `shipcheck` pre-ship gate: config parsing, CLI argument
//! parsing, and single-check execution.
use serde::{Deserialize, Serialize};
use std::process::Command;
use std::time::Instant;

/// Top-level shipcheck config: the ordered list of checks to run.
#[derive(Deserialize, Debug, Clone)]
pub struct Config {
    /// Checks run sequentially in listed order; any non-optional failure fails the gate.
    pub checks: Vec<Check>,
}

/// A single check: a human-readable `name` and the shell command `cmd` to run
/// via `sh -c`. Optional checks downgrade failures to warnings.
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Check {
    pub name: String,
    pub cmd: String,
    #[serde(default)]
    pub optional: bool,
}

/// Result of running a single check: `Pass` with its duration in seconds,
/// `Warn` when an optional check failed (duration + exit code), or `Fail`
/// with duration, the captured stderr tail, and the exit code if known.
#[derive(Debug, PartialEq)]
pub enum Outcome {
    Pass(f64),
    Warn(f64, i32),
    Fail(f64, String, Option<i32>),
}

/// Parse CLI args: returns (config_path, json_mode, quiet_mode).
/// The path is the first non-flag argument after `argv[0]`; defaults to
/// ".shipcheck.yml". `--json` anywhere switches to JSON stdout mode.
/// `--quiet` anywhere suppresses non-error output (only FAIL lines and
/// the failure summary are shown).
pub fn parse_args(args: &[String]) -> (String, bool, bool) {
    let json_mode = args.iter().any(|a| a == "--json");
    let quiet = args.iter().any(|a| a == "--quiet");
    let path = args
        .iter()
        .enumerate()
        .find(|(i, a)| *i > 0 && !a.starts_with('-'))
        .map(|(_, a)| a.clone())
        .unwrap_or_else(|| ".shipcheck.yml".to_string());
    (path, json_mode, quiet)
}

/// Parse a shipcheck config. Accepts both `{checks: [...]}` documents and
/// bare YAML lists of checks.
pub fn parse_config(raw: &str) -> Result<Config, String> {
    match serde_yaml::from_str::<Config>(raw) {
        Ok(c) => Ok(c),
        // Bare YAML list of checks (RawConfig-as-struct can't accept sequences)
        Err(_) => match serde_yaml::from_str::<Vec<Check>>(raw) {
            Ok(checks) => Ok(Config { checks }),
            Err(e) => Err(format!("invalid config: {e}")),
        },
    }
}

/// Run one check via `sh -c <cmd>`; captures duration and exit status.
pub fn run_check(check: &Check) -> (String, Outcome) {
    let start = Instant::now();
    let out = Command::new("sh").arg("-c").arg(&check.cmd).output();
    let dur = start.elapsed().as_secs_f64();
    let name = check.name.clone();
    match out {
        Ok(o) if o.status.success() => (name, Outcome::Pass(dur)),
        Ok(o) => {
            let code = o.status.code();
            if check.optional {
                (name, Outcome::Warn(dur, code.unwrap_or(-1)))
            } else {
                let tail: String = String::from_utf8_lossy(&o.stderr)
                    .lines()
                    .last()
                    .unwrap_or("")
                    .to_string();
                (name, Outcome::Fail(dur, tail, code))
            }
        }
        Err(e) => (name, Outcome::Fail(dur, format!("spawn error: {e}"), None)),
    }
}

/// One check result in machine-readable form, used for `--json` output.
/// Field presence mirrors status: `exit_code` is always emitted (null when
/// unknown), `detail` only on fail, so consumers can key on `status`.
#[derive(Serialize, Debug, PartialEq)]
pub struct CheckResult {
    pub name: String,
    pub status: &'static str,
    pub exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub duration_s: f64,
}

/// Convert ordered run results into a typed JSON report. Serialization goes
/// through serde_json rather than hand-built strings so names and failure
/// details containing quotes or backslashes are escaped correctly.
pub fn json_report(results: &[(String, Outcome)]) -> Vec<CheckResult> {
    results
        .iter()
        .map(|(name, outcome)| match outcome {
            Outcome::Pass(dur) => CheckResult {
                name: name.clone(),
                status: "pass",
                exit_code: None,
                detail: None,
                duration_s: *dur,
            },
            Outcome::Warn(dur, code) => CheckResult {
                name: name.clone(),
                status: "warn",
                exit_code: Some(*code),
                detail: None,
                duration_s: *dur,
            },
            Outcome::Fail(dur, detail, code) => CheckResult {
                name: name.clone(),
                status: "fail",
                exit_code: *code,
                detail: Some(detail.trim().to_string()),
                duration_s: *dur,
            },
        })
        .collect()
}

// ---- json_report tests ----

#[cfg(test)]
mod json_report_tests {
    use super::*;

    fn to_json(results: &[(String, Outcome)]) -> serde_json::Value {
        serde_json::to_value(json_report(results)).expect("serialize report")
    }

    #[test]
    fn json_report_pass_item() {
        let v = to_json(&[("lint".into(), Outcome::Pass(0.123))]);
        assert_eq!(v[0]["name"], "lint");
        assert_eq!(v[0]["status"], "pass");
        assert!(v[0]["exit_code"].is_null());
        assert_eq!(v[0]["duration_s"], 0.123);
        assert!(v[0].get("detail").is_none());
    }

    #[test]
    fn json_report_warn_item_keeps_exit_code() {
        let v = to_json(&[("wip".into(), Outcome::Warn(0.5, 9))]);
        assert_eq!(v[0]["status"], "warn");
        assert_eq!(v[0]["exit_code"], 9);
        assert!(v[0].get("detail").is_none());
    }

    #[test]
    fn json_report_fail_item_has_detail_and_optional_code() {
        let v = to_json(&[
            ("bad".into(), Outcome::Fail(1.0, " boom \n".into(), Some(3))),
            ("worse".into(), Outcome::Fail(2.0, "killed".into(), None)),
        ]);
        assert_eq!(v[0]["status"], "fail");
        assert_eq!(v[0]["detail"], "boom"); // trimmed
        assert_eq!(v[0]["exit_code"], 3);
        assert!(v[1]["exit_code"].is_null());
        assert_eq!(v[1]["detail"], "killed");
    }

    #[test]
    fn json_report_escapes_quotes_and_backslashes_in_names_and_details() {
        // Hand-built format! strings produced invalid JSON here; serde must not.
        let results = vec![
            (
                "say \"hi\" \\ done".to_string(),
                Outcome::Fail(0.1, "line \"quoted\" \\ backslash".to_string(), Some(1)),
            ),
            ("tab\tname".to_string(), Outcome::Pass(0.2)),
        ];
        let text = serde_json::to_string(&json_report(&results)).expect("serialize");
        let parsed: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
        assert_eq!(parsed[0]["name"], "say \"hi\" \\ done");
        assert_eq!(parsed[0]["detail"], "line \"quoted\" \\ backslash");
        assert_eq!(parsed[1]["name"], "tab\tname");
    }

    #[test]
    fn json_report_preserves_order_of_input() {
        let results = vec![
            ("a".to_string(), Outcome::Pass(0.01)),
            ("b".to_string(), Outcome::Warn(0.02, 4)),
            ("c".to_string(), Outcome::Fail(0.03, "x".into(), Some(1))),
        ];
        let v = to_json(&results);
        assert_eq!(v.as_array().unwrap().len(), 3,);
        assert_eq!(v[0]["name"], "a");
        assert_eq!(v[1]["name"], "b");
        assert_eq!(v[2]["name"], "c");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk(name: &str, cmd: &str) -> String {
        format!("{{ name: {name}, cmd: {cmd} }}")
    }

    // ---- parse_args ----

    #[test]
    fn args_default_path_and_no_json() {
        let argv = vec!["shipcheck".to_string()];
        let (path, json, quiet) = parse_args(&argv);
        assert_eq!(path, ".shipcheck.yml");
        assert!(!json);
        assert!(!quiet);
    }

    #[test]
    fn args_explicit_path() {
        let argv = vec!["shipcheck".to_string(), "custom.yml".to_string()];
        assert_eq!(parse_args(&argv).0, "custom.yml");
    }

    #[test]
    fn args_json_flag_detected_anywhere() {
        let argv = vec![
            "shipcheck".to_string(),
            "--json".to_string(),
            "cfg.yml".to_string(),
        ];
        let (path, json, _) = parse_args(&argv);
        assert!(json);
        assert_eq!(path, "cfg.yml");
    }

    #[test]
    fn args_quiet_flag_detected_anywhere() {
        let argv = vec![
            "shipcheck".to_string(),
            "cfg.yml".to_string(),
            "--quiet".to_string(),
        ];
        let (path, _, quiet) = parse_args(&argv);
        assert!(quiet);
        assert_eq!(path, "cfg.yml");
    }

    #[test]
    fn args_quiet_and_json_can_combine() {
        let argv = vec![
            "shipcheck".to_string(),
            "--quiet".to_string(),
            "--json".to_string(),
        ];
        let (_, json, quiet) = parse_args(&argv);
        assert!(json);
        assert!(quiet);
    }

    #[test]
    fn args_flags_are_not_paths() {
        let argv = vec![
            "shipcheck".to_string(),
            "--json".to_string(),
            "-x".to_string(),
        ];
        let (path, _, _) = parse_args(&argv);
        assert_eq!(path, ".shipcheck.yml");
    }

    // ---- parse_config ----

    #[test]
    fn config_checks_document() {
        let cfg = parse_config(
            "checks:\n  - name: lint\n    cmd: echo hi\n  - name: fmt\n    cmd: true\n",
        )
        .expect("should parse");
        assert_eq!(cfg.checks.len(), 2);
        assert_eq!(cfg.checks[0].name, "lint");
        assert!(!cfg.checks[0].optional); // serde default
    }

    #[test]
    fn config_bare_list_accepted() {
        let cfg = parse_config("- name: t\n  cmd: true\n").expect("bare list should parse");
        assert_eq!(cfg.checks.len(), 1);
        assert_eq!(cfg.checks[0].name, "t");
    }

    #[test]
    fn config_optional_flag_parsed() {
        let cfg =
            parse_config("checks:\n  - name: wip\n    cmd: false\n    optional: true\n").unwrap();
        assert!(cfg.checks[0].optional);
    }

    #[test]
    fn config_empty_document_yields_zero_checks() {
        // serde_yaml parses "" as null -> Config with no checks;
        // main's emptiness check turns this into "no checks defined" exit 2.
        let cfg = parse_config("").unwrap();
        assert_eq!(cfg.checks.len(), 0);
    }

    #[test]
    fn config_unknown_map_without_checks_is_err() {
        // A mapping document that lacks `checks` is rejected outright
        // (main would print "invalid config" and exit 2).
        assert!(parse_config("other: stuff\n").is_err());
    }

    #[test]
    fn config_malformed_yaml_is_err() {
        let raw = "checks:\n  - name: [unclosed\n    cmd: {{{\n";
        assert!(parse_config(raw).is_err());
    }

    #[test]
    fn config_wrong_types_are_err() {
        // `checks` is not a list / missing required fields
        assert!(parse_config("checks: 42\n").is_err());
        assert!(parse_config("checks:\n  - cmd: true\n").is_err()); // no name
        assert!(parse_config("checks:\n  - name: x\n").is_err()); // no cmd
    }

    #[test]
    fn config_duplicate_names_preserved() {
        let raw = format!(
            "checks:\n  - {}\n  - {}\n",
            mk("\"dup\"", "\"true\""),
            mk("\"dup\"", "\"false\"")
        );
        let cfg = parse_config(&raw).unwrap();
        assert_eq!(cfg.checks.len(), 2);
        assert_eq!(cfg.checks[0].name, cfg.checks[1].name);
    }

    // ---- run_check ----

    #[test]
    fn run_pass_on_zero_exit() {
        let c = Check {
            name: "ok".into(),
            cmd: "true".into(),
            optional: false,
        };
        let (name, out) = run_check(&c);
        assert_eq!(name, "ok");
        assert!(matches!(out, Outcome::Pass(d) if d >= 0.0));
    }

    #[test]
    fn run_fail_records_stderr_tail_and_code() {
        let c = Check {
            name: "bad".into(),
            cmd: "echo line1 >&2; echo boom >&2; exit 3".into(),
            optional: false,
        };
        match run_check(&c).1 {
            Outcome::Fail(_, detail, Some(code)) => {
                assert_eq!(code, 3);
                assert_eq!(detail, "boom"); // last stderr line only
            }
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn run_fail_with_empty_stderr_has_empty_detail() {
        let c = Check {
            name: "quiet".into(),
            cmd: "exit 1".into(),
            optional: false,
        };
        match run_check(&c).1 {
            Outcome::Fail(_, detail, Some(1)) => assert_eq!(detail, ""),
            other => panic!("expected Fail(.,\"\",Some(1)), got {other:?}"),
        }
    }

    #[test]
    fn run_optional_failure_warns_with_code() {
        let c = Check {
            name: "wip".into(),
            cmd: "exit 7".into(),
            optional: true,
        };
        match run_check(&c).1 {
            Outcome::Warn(_, 7) => {}
            other => panic!("expected Warn(_,7), got {other:?}"),
        }
    }

    #[test]
    fn run_spawn_error_fails_without_code() {
        // command name cannot be spawned through sh? sh always exists, so use
        // an exec failure: sh -c 'nonexistent-cmd-xyz' exits 127 (a Fail with code).
        // To hit the Err branch we'd need spawn() itself to fail, which sh -c
        // essentially never does; instead verify the 127 path surfaces as Fail.
        let c = Check {
            name: "missing".into(),
            cmd: "definitely-not-a-real-command-xyz123".into(),
            optional: false,
        };
        match run_check(&c).1 {
            Outcome::Fail(_, _, code) => assert_ne!(code, Some(0)),
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn run_multiline_success_passes() {
        let c = Check {
            name: "multi".into(),
            cmd: "echo one; echo two; exit 0".into(),
            optional: false,
        };
        assert!(matches!(run_check(&c).1, Outcome::Pass(_)));
    }
}
