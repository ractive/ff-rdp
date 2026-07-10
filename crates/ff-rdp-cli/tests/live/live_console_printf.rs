//! Live test: `live_console_printf_e2e` (iter-78 AC3).
//!
//! Verifies iter-77 Theme C — printf-style substitution in
//! `parse_console_resources`.
//!
//! The test:
//! 1. Self-launches headless Firefox on a random port and navigates to
//!    `about:blank`.
//! 2. Runs `ff-rdp eval 'console.log("hello %s, you are %d <marker>", "world", 42)'`
//!    (with a per-run unique marker token) to emit a formatted console message.
//! 3. Waits briefly for the message to be buffered.
//! 4. Runs `ff-rdp console --pattern '<marker>'` and asserts that at least one
//!    result has `message == "hello world, you are 42 <marker>"`.
//!
//! # iter-114 status: LEFT RED — product-side gap, not a test-fixture issue
//!
//! This test was ALREADY self-launching (no hardcoded-default-port dependency)
//! before iter-114; the iter-110 sweep recorded it red regardless. Diagnosis:
//!
//! - `commands::console::run` (`crates/ff-rdp-cli/src/commands/console.rs`)
//!   calls `WebConsoleActor::get_cached_messages` directly — it never calls
//!   `WebConsoleActor::start_listeners` first.
//! - Per the Firefox WebConsole actor protocol (documented at
//!   `kb/rdp/actors/console.md`, `getCachedMessages` section, sourced from
//!   `devtools/server/actors/webconsole.js`): `getCachedMessages` returns
//!   only messages recorded **since `startListeners` was called** on that
//!   actor. Before `startListeners` ever runs, the server-side cache buffer
//!   for that target is simply never populated — `getCachedMessages` legally
//!   returns `{ messages: [] }` no matter how recently a `console.log` ran.
//! - Verified live (2026-07-10, Firefox 152.0.5): with the fixture/flow as
//!   written, `console --pattern hello` after the `eval` returns
//!   `results: []` (`total: 0`) every time — even for a plain page-script
//!   `<script>console.log(...)</script>` with no `eval`/CSP involvement at
//!   all, ruling out a printf-substitution or eval-path bug.
//! - Confirmed the mechanism directly: temporarily patching
//!   `commands::console::run` to call `start_listeners(["PageError","ConsoleAPI"])`
//!   before `get_cached_messages` (experiment only, NOT committed — reverted
//!   before finishing this file) made the exact same eval-then-console
//!   sequence pass, returning the printf-substituted message correctly. This
//!   isolates the gap to the missing `start_listeners` call in the CLI
//!   command — the printf substitution itself (`parse_console_resources`,
//!   the iter-77 Theme C fix this test targets) works correctly once the
//!   cache is actually primed.
//! - Every currently-available CLI entry point that reaches `start_listeners`
//!   is `console --follow` (`commands::console::run_follow`), which uses the
//!   unrelated Watcher-based `watchResources(console-message, error-message)`
//!   subscription (not the legacy `startListeners`/`getCachedMessages` pair)
//!   and blocks forever streaming — it does not prime the legacy cache either
//!   (verified live: priming with a `--follow` burst before `eval` does not
//!   help). There is therefore no test-side sequencing that can make a plain,
//!   short-lived `console` invocation see a message logged by an earlier,
//!   separate `--no-daemon` connection: only a product change (calling
//!   `start_listeners` inside `commands::console::run`, mirroring what
//!   `run_follow` already implies via `watchResources`) can fix this.
//! - Per this iteration's scope, product code under `crates/*/src/` must not
//!   be modified here — this test is intentionally left red pending a
//!   product-side fix in `commands::console::run`.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live live_console_printf -- --nocapture

use std::process::{Command, Output};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::common::{LiveFirefox, base_args, ff_rdp_bin, live_tests_enabled};

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
/// Emit `console.log("hello %s, you are %d <marker>", "world", 42)` via eval
/// (with a per-run unique marker so cache pollution across repeated runs
/// cannot cause a false match), then read it back via
/// `ff-rdp console --pattern '<marker>'` and assert that the `message` field
/// was printf-substituted to `"hello world, you are 42 <marker>"`.
///
/// See the module-level doc comment: this is currently LEFT RED by design —
/// see the "iter-114 status" section above for the full product-side
/// diagnosis (`commands::console::run` never calls `start_listeners`).
///
/// Gated on `FF_RDP_LIVE_TESTS=1`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_console_printf_e2e() {
    if !live_tests_enabled() {
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

    // Unique per-run marker so cache pollution across repeated runs / other
    // console output cannot cause a false match or mask a missed match.
    let marker = format!(
        "m{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default()
    );

    // Emit a printf-style console.log.  The format specifiers %s and %d must be
    // substituted by `parse_console_resources` (iter-77 Theme C) to produce:
    //   "hello world, you are 42 <marker>"
    let script = format!(r#"console.log("hello %s, you are %d {marker}", "world", 42)"#);
    let eval_out = Command::new(ff_rdp_bin())
        .args(ff_args())
        .args(["eval", &script])
        .output()
        .expect("eval console.log printf");

    assert!(
        eval_out.status.success(),
        "live_console_printf_e2e: eval exited non-zero — {}",
        String::from_utf8_lossy(&eval_out.stderr)
    );

    // Give Firefox time to buffer the console message before we read it back.
    std::thread::sleep(Duration::from_millis(500));

    // Read console messages, filtered to those containing the unique marker.
    let console_out = Command::new(ff_rdp_bin())
        .args(ff_args())
        .args(["console", "--pattern", &marker])
        .output()
        .expect("ff-rdp console --pattern <marker>");

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

    let expected = format!("hello world, you are 42 {marker}");

    let found = results
        .iter()
        .any(|r| r["message"].as_str().is_some_and(|m| m == expected));

    assert!(
        found,
        "live_console_printf_e2e: expected a console message with \
         message == {expected:?} but got:\n{}\n\
         See this file's module-level doc comment (\"iter-114 status\") for the \
         product-side root-cause diagnosis if this is failing: \
         commands::console::run never calls WebConsoleActor::start_listeners, \
         so getCachedMessages legitimately returns nothing for messages logged \
         before listeners were ever started on this actor.",
        serde_json::to_string_pretty(results).unwrap_or_default()
    );

    eprintln!("live_console_printf_e2e: PASS — printf substitution round-trip confirmed");
}
