//! Live test: `live_console_printf_e2e` (iter-78 AC3).
//!
//! Verifies iter-77 Theme C — printf-style substitution in
//! `parse_console_resources`.
//!
//! The test:
//! 1. Navigates to `about:blank`.
//! 2. Runs `ff-rdp eval 'console.log("hello %s, you are %d", "world", 42)'`
//!    to emit a formatted console message.
//! 3. Waits briefly for the message to be buffered.
//! 4. Runs `ff-rdp console --pattern 'hello'` and asserts that at least one
//!    result has `message == "hello world, you are 42"`.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live_console_printf -- --nocapture

#[path = "common/mod.rs"]
mod common;

use std::process::{Command, Output};
use std::time::Duration;

use common::{LiveFirefox, base_args, ff_rdp_bin};

fn parse_json(output: &Output) -> serde_json::Value {
    let s = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(s.trim()).unwrap_or_else(|e| {
        panic!(
            "stdout is not valid JSON: {e}\nstdout={s}\nstderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

/// `live_console_printf_e2e`:
/// Emit `console.log("hello %s, you are %d", "world", 42)` via eval, then
/// read it back via `ff-rdp console --pattern 'hello'` and assert that the
/// `message` field was printf-substituted to `"hello world, you are 42"`.
///
/// Gated on `FF_RDP_LIVE_TESTS=1`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_console_printf_e2e() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_console_printf_e2e: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_console_printf_e2e: Firefox not available — skipping");
        return;
    };

    let ff_args = || base_args(ff.port());

    // Navigate to about:blank for a clean console.
    let nav = Command::new(ff_rdp_bin())
        .args(ff_args())
        .args(["navigate", "about:blank"])
        .output()
        .expect("navigate to about:blank");

    assert!(
        nav.status.success(),
        "live_console_printf_e2e: navigate failed — {}",
        String::from_utf8_lossy(&nav.stderr)
    );

    // Emit a printf-style console.log.  The format specifiers %s and %d must be
    // substituted by `parse_console_resources` (iter-77 Theme C) to produce:
    //   "hello world, you are 42"
    let eval_out = Command::new(ff_rdp_bin())
        .args(ff_args())
        .args([
            "eval",
            r#"console.log("hello %s, you are %d", "world", 42)"#,
        ])
        .output()
        .expect("eval console.log printf");

    assert!(
        eval_out.status.success(),
        "live_console_printf_e2e: eval exited non-zero — {}",
        String::from_utf8_lossy(&eval_out.stderr)
    );

    // Give Firefox time to buffer the console message before we read it back.
    std::thread::sleep(Duration::from_millis(500));

    // Read console messages, filtered to those containing "hello".
    let console_out = Command::new(ff_rdp_bin())
        .args(ff_args())
        .args(["console", "--pattern", "hello"])
        .output()
        .expect("ff-rdp console --pattern hello");

    assert!(
        console_out.status.success(),
        "live_console_printf_e2e: console command exited non-zero — {}",
        String::from_utf8_lossy(&console_out.stderr)
    );

    let json = parse_json(&console_out);

    let results = json["results"]
        .as_array()
        .expect("console results must be an array");

    eprintln!(
        "live_console_printf_e2e: got {} matching console message(s)",
        results.len()
    );

    let expected = "hello world, you are 42";

    let found = results
        .iter()
        .any(|r| r["message"].as_str().is_some_and(|m| m == expected));

    assert!(
        found,
        "live_console_printf_e2e: expected a console message with \
         message == {expected:?} but got:\n{}",
        serde_json::to_string_pretty(results).unwrap_or_default()
    );

    eprintln!("live_console_printf_e2e: PASS — printf substitution round-trip confirmed");
}
