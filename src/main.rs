// shipcheck v0.2 — reads .shipcheck.yml, runs each check in parallel,
// reports pass/fail with durations in table or JSON form.
// Core logic lives in the library crate (src/lib.rs) so it is unit-tested.
use shipcheck::{parse_args, parse_config, run_check, Outcome};
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
    let mut json_items: Vec<String> = Vec::new();

    for (_, (name, outcome)) in &results {
        match outcome {
            Outcome::Pass(dur) => {
                out!("  PASS  {name:<24} ({dur:.2?})");
                json_items.push(format!(
                    r#"{{"name":{name:?},"status":"pass","duration_s":{dur:.3}}}"#
                ));
            }
            Outcome::Warn(dur, code) => {
                out!("  WARN  {name:<24} ({dur:.2?}) exit {code}");
                json_items.push(format!(r#"{{"name":{name:?},"status":"warn","exit_code":{code},"duration_s":{dur:.3}}}"#));
            }
            Outcome::Fail(dur, detail, code) => {
                failed += 1;
                let extra = match code {
                    Some(c) => format!(" exit {c}"),
                    None => String::new(),
                };
                err_out!("  FAIL  {name:<24}{extra} {}", detail.trim());
                let code_json = match code {
                    Some(c) => c.to_string(),
                    None => "null".to_string(),
                };
                json_items.push(format!(
                    r#"{{"name":{name:?},"status":"fail","exit_code":{code_json},"duration_s":{dur:.3},"detail":{detail:?}}}"#
                ));
            }
        }
    }

    let total = started.elapsed().as_secs_f64();
    if json_mode {
        println!("[\n  {}\n]", json_items.join(",\n  "));
    } else {
        if !quiet {
            println!();
        }
    }
    if failed == 0 {
        out!("✅ all checks passed — ship it ({total:.2}s wall)");
    } else {
        err_out!("❌ {failed} check(s) failed — fix before pushing ({total:.2}s wall)");
        std::process::exit(1);
    }
}
