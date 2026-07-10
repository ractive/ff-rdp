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
//! # iter-116 status: GREEN — product-side gap fixed
//!
//! This test was LEFT RED through iter-114 pending a product fix, then
//! un-redded in iter-116 once that fix landed. Root cause and resolution:
//!
//! - Before iter-116, `commands::console::run`
//!   (`crates/ff-rdp-cli/src/commands/console.rs`) called
//!   `WebConsoleActor::get_cached_messages` directly — it never called
//!   `WebConsoleActor::start_listeners` first.
//! - Per the Firefox WebConsole actor protocol (documented at
//!   `kb/rdp/actors/console.md`, `getCachedMessages` section, sourced from
//!   `devtools/server/actors/webconsole.js`): `getCachedMessages` returns
//!   only messages recorded **since `startListeners` was called** on that
//!   actor. Before `startListeners` ever runs, the server-side cache buffer
//!   for that target is simply never populated — `getCachedMessages` legally
//!   returns `{ messages: [] }` no matter how recently a `console.log` ran.
//! - Verified live (2026-07-10, Firefox 152.0.5): with the previous flow,
//!   `console --pattern hello` after the `eval` returned `results: []`
//!   (`total: 0`) every time — even for a plain page-script
//!   `<script>console.log(...)</script>` with no `eval`/CSP involvement at
//!   all, ruling out a printf-substitution or eval-path bug.
//! - iter-116 fix: `commands::console::run` now calls
//!   `start_listeners(["PageError","ConsoleAPI"])` (via the private
//!   `prime_console_cache` helper) *before* `get_cached_messages`, priming the
//!   cache so a fresh `--no-daemon` connection sees a message an earlier,
//!   separate `eval` connection logged. The printf substitution itself
//!   (`parse_console_resources`, the iter-77 Theme C fix this test targets)
//!   was already correct once the cache is actually primed.
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
/// See the module-level doc comment ("iter-116 status"): this is now GREEN —
/// `commands::console::run` primes the cache via `start_listeners` before
/// reading it.
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
         Regression check: `commands::console::run` must call \
         `WebConsoleActor::start_listeners` (via `prime_console_cache`) before \
         `getCachedMessages`, otherwise Firefox returns nothing for messages \
         logged before listeners were ever started on this actor. See this \
         file's module-level doc comment (\"iter-116 status\").",
        serde_json::to_string_pretty(results).unwrap_or_default()
    );

    eprintln!("live_console_printf_e2e: PASS — printf substitution round-trip confirmed");
}
