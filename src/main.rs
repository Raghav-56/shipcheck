// shipcheck v0.2 — reads .shipcheck.yml, runs each check in parallel,
// reports pass/fail with durations in table or JSON form.
// Core logic lives in the library crate (src/lib.rs) so it is unit-tested.
use shipcheck::{json_report, parse_args, parse_config, run_check, Outcome};
use std::sync::mpsc;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // `--version` / `-V` prints the crate version from Cargo and exits.
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("shipcheck {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    let (path, json_mode, quiet) = parse_args(&args);

    // In --json mode the table goes to stderr; stdout carries only pure JSON.
    // In --quiet mode non-error output (header, PASS/WARN lines, the success
    // summary) is suppressed entirely; FAIL lines and the failure summary
    // still print so errors remain visible.
    macro_rules! out {
        ($($arg:tt)*) => {
            if json_mode { eprintln!($($arg)*) } else if !quiet { println!($($arg)*) }
        };
    }
    macro_rules! err_out {
        ($($arg:tt)*) => {
            if json_mode { eprintln!($($arg)*) } else { println!($($arg)*) }
        };
    }

    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("shipcheck: cannot read {path}: {e}");
            std::process::exit(2);
        }
    };

    let config: shipcheck::Config = match parse_config(&raw) {
        Ok(c) if !c.checks.is_empty() => c,
        Ok(_) => {
            eprintln!("shipcheck: no checks defined in {path}");
            std::process::exit(2);
        }
        Err(e) => {
            eprintln!("shipcheck: {e}");
            std::process::exit(2);
        }
    };

    out!(
        "shipcheck: running {} checks in parallel\n",
        config.checks.len()
    );
    let started = Instant::now();

    let (tx, rx) = mpsc::channel();
    // Scoped threads borrow the checks instead of cloning each one into a
    // 'static thread (avoids N String clones of name+cmd per run). Each
    // worker sends its config index so result ordering is O(n log n).
    std::thread::scope(|s| {
        for (idx, check) in config.checks.iter().enumerate() {
            let tx = tx.clone();
            s.spawn(move || {
                tx.send((idx, run_check(check))).expect("send result");
            });
        }
    });
    drop(tx);
    // Preserve original config order regardless of completion order.
    let mut results: Vec<(usize, (String, Outcome))> = rx.iter().collect();
    results.sort_by_key(|(idx, _)| *idx);

    let mut failed = 0usize;

    for (_, (name, outcome)) in &results {
        match outcome {
            Outcome::Pass(dur) => {
                out!("  PASS  {name:<24} ({dur:.2?})");
            }
            Outcome::Warn(dur, code) => {
                out!("  WARN  {name:<24} ({dur:.2?}) exit {code}");
            }
            Outcome::Fail(_, detail, code) => {
                failed += 1;
                let extra = match code {
                    Some(c) => format!(" exit {c}"),
                    None => String::new(),
                };
                err_out!("  FAIL  {name:<24}{extra} {}", detail.trim());
            }
        }
    }

    if json_mode {
        // Typed serialization via serde_json: names and failure details are
        // escaped correctly even when they contain quotes or backslashes.
        let report: Vec<(String, Outcome)> = results.into_iter().map(|(_, r)| r).collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&json_report(&report)).expect("serialize report")
        );
    } else if !quiet {
        println!();
    }
    let total = started.elapsed().as_secs_f64();
    if failed == 0 {
        out!("✅ all checks passed — ship it ({total:.2}s wall)");
    } else {
        err_out!("❌ {failed} check(s) failed — fix before pushing ({total:.2}s wall)");
        std::process::exit(1);
    }
}
