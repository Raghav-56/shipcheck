// shipcheck v0.1 — reads .shipcheck.yml, runs each command, reports pass/fail with durations.
// v0.2 plan: parallel execution via threads, secret-scan, JSON output mode.
use serde::Deserialize;
use std::process::Command;
use std::time::Instant;

#[derive(Deserialize)]
struct Config {
    checks: Vec<Check>,
}

#[derive(Deserialize)]
struct Check {
    name: String,
    cmd: String,
    #[serde(default)]
    optional: bool,
}

#[derive(Deserialize)]
struct RawConfig {
    #[serde(default)]
    checks: Vec<Check>,
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| ".shipcheck.yml".to_string());

    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("shipcheck: cannot read {path}: {e}");
            std::process::exit(2);
        }
    };

    // Accept both a bare list and a {checks: [...]} document
    let config: Config = match serde_yaml::from_str::<Config>(&raw) {
        Ok(c) => c,
        Err(_) => match serde_yaml::from_str::<RawConfig>(&raw) {
            Ok(r) => Config { checks: r.checks },
            Err(e) => {
                eprintln!("shipcheck: invalid config: {e}");
                std::process::exit(2);
            }
        },
    };

    if config.checks.is_empty() {
        eprintln!("shipcheck: no checks defined in {path}");
        std::process::exit(2);
    }

    let mut failed = 0usize;
    println!("shipcheck: running {} checks\n", config.checks.len());

    for check in &config.checks {
        let start = Instant::now();
        let status = Command::new("sh").arg("-c").arg(&check.cmd).status();
        let dur = start.elapsed();
        match status {
            Ok(s) if s.success() => {
                println!("  PASS  {:<24} ({:.2?})", check.name, dur);
            }
            Ok(s) => {
                let tag = if check.optional { "WARN" } else { "FAIL" };
                if !check.optional {
                    failed += 1;
                }
                println!("  {}  {:<24} ({:.2?}) exit {}", tag, check.name, dur, s.code().unwrap_or(-1));
            }
            Err(e) => {
                failed += 1;
                println!("  FAIL  {:<24} spawn error: {}", check.name, e);
            }
        }
    }

    println!();
    if failed == 0 {
        println!("✅ all checks passed — ship it");
    } else {
        println!("❌ {failed} check(s) failed — fix before pushing");
        std::process::exit(1);
    }
}
