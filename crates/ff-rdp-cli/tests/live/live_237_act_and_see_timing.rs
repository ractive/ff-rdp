//! Live tests for iteration 237 — act-and-see *timing*.
//!
//! Two defects, one branch, both about a command spending the wrong amount of
//! time deciding what happened:
//!
//! - **Part A.** `type --submit` reported `results.navigated: false` on a
//!   submission that really did navigate. The post-`requestSubmit()`
//!   `navigated_away` poll shared the 600 ms constant sized for the *local*
//!   post-Enter check, and a real network round-trip outlives it — so the
//!   envelope carried `navigated: false` next to a `results.page` collected
//!   from the destination. Two fixtures, because the grace period turned out
//!   not to be the whole fix: `/slow` holds its first byte back for
//!   [`DESTINATION_DELAY`] (longer than the old 600 ms budget, inside the new
//!   3 s one), while `/slower` holds it for [`GRACE_OUTLIVING_DELAY`], past
//!   the new budget as well — so only the refreshed-target re-check can
//!   answer the second one. Both fail deterministically on `main`.
//!
//! - **Part B** (absorbed iter-238). A guessed selector that matches nothing
//!   cost the whole `--timeout` before `click` said "0 elements matched".
//!   Two tests, one per side of the discrimination the fix has to make:
//!   an idle page answers early, a page with a request in flight keeps its
//!   full budget.
//!
//! daemon-parity: these run the daemon route, like the iter-210/220 suites
//! they extend.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live live_237_act_and_see_timing -- --nocapture

use std::collections::HashMap;
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::common::{FixtureRoute, FixtureServer, LiveFirefox, ff_rdp_bin, live_tests_enabled};

/// How long `/slow` holds its response back before the first byte.
///
/// Deliberately above the 600 ms post-Enter grace period the post-submit poll
/// used to share (Part A's root cause) and well below the 3 s budget it has
/// now, so the assertion below separates the two implementations rather than
/// racing them.
const DESTINATION_DELAY: Duration = Duration::from_millis(1_200);

/// The auto-wait budget every test here passes explicitly.
///
/// The same 10 s default that made a guessed selector read as a hang. Named so
/// the Part B assertions can be stated as fractions of it rather than as bare
/// numbers.
const CLICK_TIMEOUT_MS: u64 = 10_000;

fn daemon_args(port: u16) -> Vec<String> {
    vec![
        "--host".to_owned(),
        "127.0.0.1".to_owned(),
        "--port".to_owned(),
        port.to_string(),
        "--timeout".to_owned(),
        CLICK_TIMEOUT_MS.to_string(),
    ]
}

fn stop_daemon(port: u16) {
    let _ = Command::new(ff_rdp_bin())
        .args(["--host", "127.0.0.1", "--port", &port.to_string()])
        .args(["daemon", "stop"])
        .output();
}

fn firefox_with_daemon(test: &str) -> LiveFirefox {
    let ff = LiveFirefox::headless_on_random_port();
    assert!(
        ff.with_daemon().is_some(),
        "{test}: the proxy daemon did not start for Firefox on port {}",
        ff.port()
    );
    ff
}

fn run(port: u16, args: &[&str]) -> Output {
    Command::new(ff_rdp_bin())
        .args(daemon_args(port))
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("spawn ff-rdp {args:?}: {e}"))
}

fn run_json(port: u16, args: &[&str]) -> Value {
    let out = run(port, args);
    assert!(
        out.status.success(),
        "command {args:?} failed: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("output for {args:?} not JSON: {e}\n{stdout}"))
}

fn first_heading(action: &Value) -> String {
    action["results"]["page"]["headings"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("results.page.headings[0].text missing: {action}"))
        .to_owned()
}

// ---------------------------------------------------------------------------
// Part A — `type --submit` must not under-report `navigated`
// ---------------------------------------------------------------------------

/// `/` carries a form whose action is the slow-to-commit `/slow`.
fn slow_form_fixture() -> HashMap<String, FixtureRoute> {
    let mut routes = HashMap::new();
    routes.insert(
        "/".to_owned(),
        FixtureRoute::html(
            "<!doctype html><title>t237 origin</title><body>\
             <h1>Ada Lovelace</h1>\
             <form action=\"/slow\" method=\"get\">\
             <input name=\"q\" aria-label=\"Search\">\
             </form></body>",
        ),
    );
    routes.insert(
        "/slow".to_owned(),
        FixtureRoute::html(
            "<!doctype html><title>t237 destination</title><body>\
             <h1>Charles Babbage</h1></body>",
        )
        .with_delay(DESTINATION_DELAY),
    );
    routes
}

/// AC: `results.navigated` must agree with `results.page`.
///
/// The bug was not "false is the wrong value" in the abstract — it was one
/// envelope contradicting itself, which a caller reading only `navigated` (no
/// `--with-page`) has no way to notice. So the assertion is deliberately
/// stated as an agreement between the two fields: the heading proves a
/// cross-document navigation happened, therefore `navigated` must be `true`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_237_submit_navigated_agrees_with_the_page_it_reports() {
    if !live_tests_enabled() {
        eprintln!(
            "live_237_submit_navigated_agrees_with_the_page_it_reports: set FF_RDP_LIVE_TESTS=1"
        );
        return;
    }
    let ff = firefox_with_daemon("live_237_submit_navigated_agrees_with_the_page_it_reports");
    let port = ff.port();
    let Some(server) = FixtureServer::start(slow_form_fixture()) else {
        eprintln!(
            "live_237_submit_navigated_agrees_with_the_page_it_reports: no fixture HTTP — skipping"
        );
        stop_daemon(port);
        return;
    };

    run_json(port, &["navigate", &server.base_url()]);
    let typed = run_json(
        port,
        &[
            "type",
            "input[name=q]",
            "babbage",
            "--submit",
            "--with-page",
        ],
    );

    assert_eq!(
        typed["results"]["submitted"], true,
        "type --submit must report submitted: {typed}"
    );
    assert_eq!(
        first_heading(&typed),
        "Charles Babbage",
        "--with-page must report the destination the submission produced: {typed}"
    );
    assert_eq!(
        typed["results"]["navigated"], true,
        "results.navigated must agree with results.page — the heading above is \
         proof of a cross-document navigation, so `false` here is one envelope \
         contradicting itself: {typed}"
    );

    stop_daemon(port);
}

/// The delay that outlives the post-`requestSubmit()` grace period entirely,
/// so this fixture exercises the *recovery* rather than the widened budget.
///
/// Above [`REQUEST_SUBMIT_NAVIGATION_GRACE_MS`]'s 3 s, so
/// `navigated_away`'s poll is guaranteed to give up before the destination
/// commits and the only thing that can answer `navigated` correctly is
/// `navigated_after_refresh` re-reading `location.href` off the new target.
/// Still well inside the 10 s `--timeout` this suite passes, so `--with-page`
/// has room to collect the destination for the agreement assertion.
const GRACE_OUTLIVING_DELAY: Duration = Duration::from_millis(4_500);

/// `/` carries a form whose action commits only after the whole grace period
/// has expired.
fn very_slow_form_fixture() -> HashMap<String, FixtureRoute> {
    let mut routes = HashMap::new();
    routes.insert(
        "/".to_owned(),
        FixtureRoute::html(
            "<!doctype html><title>t237 origin</title><body>\
             <h1>Ada Lovelace</h1>\
             <form action=\"/slower\" method=\"get\">\
             <input name=\"q\" aria-label=\"Search\">\
             </form></body>",
        ),
    );
    routes.insert(
        "/slower".to_owned(),
        FixtureRoute::html(
            "<!doctype html><title>t237 destination</title><body>\
             <h1>Grace Hopper</h1></body>",
        )
        .with_delay(GRACE_OUTLIVING_DELAY),
    );
    routes
}

/// AC, the harder half: the two fields must still agree when the destination
/// takes longer to commit than the post-`requestSubmit()` grace period.
///
/// This is the case that showed the grace period was never the whole fix.
/// Measured against Wikipedia while implementing this iteration, a longer
/// budget changed nothing: Firefox stops answering `evaluateJSAsync` on the
/// pre-submit console actor while it commits the new document, so the poll's
/// first iteration blocks on the socket read for the *transport's* deadline
/// (`--timeout`) and the loop never completes a probe before its own deadline
/// has passed. Only re-resolving the target and reading `location.href` off
/// the document that now exists gets the right answer — and that is what this
/// test pins, because [`DESTINATION_DELAY`]'s 1.2 s is short enough that the
/// widened grace period alone could have carried the test above.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_237_submit_navigated_survives_a_destination_slower_than_the_grace() {
    if !live_tests_enabled() {
        eprintln!(
            "live_237_submit_navigated_survives_a_destination_slower_than_the_grace: \
             set FF_RDP_LIVE_TESTS=1"
        );
        return;
    }
    let ff = firefox_with_daemon(
        "live_237_submit_navigated_survives_a_destination_slower_than_the_grace",
    );
    let port = ff.port();
    let Some(server) = FixtureServer::start(very_slow_form_fixture()) else {
        eprintln!(
            "live_237_submit_navigated_survives_a_destination_slower_than_the_grace: \
             no fixture HTTP — skipping"
        );
        stop_daemon(port);
        return;
    };

    run_json(port, &["navigate", &server.base_url()]);
    let typed = run_json(
        port,
        &["type", "input[name=q]", "hopper", "--submit", "--with-page"],
    );

    assert_eq!(
        first_heading(&typed),
        "Grace Hopper",
        "--with-page must report the destination even when it commits late: {typed}"
    );
    assert_eq!(
        typed["results"]["navigated"], true,
        "the grace period cannot have observed this navigation — the refreshed-target \
         re-check is what must report it, and `false` here means the envelope is back \
         to contradicting itself: {typed}"
    );

    stop_daemon(port);
}

/// A form whose `submit` handler cancels the submission — the AJAX shape.
fn cancelled_form_fixture() -> HashMap<String, FixtureRoute> {
    let mut routes = HashMap::new();
    routes.insert(
        "/".to_owned(),
        FixtureRoute::html(
            "<!doctype html><title>t237 ajax</title><body>\
             <h1>Ada Lovelace</h1>\
             <form action=\"/slower\" method=\"get\">\
             <input name=\"q\" aria-label=\"Search\">\
             </form>\
             <script>document.querySelector('form')\
             .addEventListener('submit', function(e) { e.preventDefault(); });</script>\
             </body>",
        ),
    );
    routes
}

/// The latency guard on the fix above: a submission the page *cancels* must
/// still answer fast.
///
/// `navigated_after_refresh` polls for whatever is left of `--timeout`, which
/// is right for a load that is genuinely in flight and badly wrong for a form
/// that will never navigate — every AJAX form would have gone from ~600 ms to
/// the full 10 s. `build_request_submit_js` reports `preventDefault()` on the
/// `submit` event as `cancelled`, and `press_enter_and_submit` skips the
/// extended poll on that signal. This test is what keeps that gate honest: it
/// fails on a 10 s stall, which is exactly what removing the gate produces.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_237_cancelled_submit_does_not_wait_out_the_timeout() {
    if !live_tests_enabled() {
        eprintln!(
            "live_237_cancelled_submit_does_not_wait_out_the_timeout: set FF_RDP_LIVE_TESTS=1"
        );
        return;
    }
    let ff = firefox_with_daemon("live_237_cancelled_submit_does_not_wait_out_the_timeout");
    let port = ff.port();
    let Some(server) = FixtureServer::start(cancelled_form_fixture()) else {
        eprintln!(
            "live_237_cancelled_submit_does_not_wait_out_the_timeout: no fixture HTTP — skipping"
        );
        stop_daemon(port);
        return;
    };

    run_json(port, &["navigate", &server.base_url()]);
    let started = Instant::now();
    let typed = run_json(port, &["type", "input[name=q]", "lovelace", "--submit"]);
    let elapsed = started.elapsed();

    assert_eq!(
        typed["results"]["navigated"], false,
        "a cancelled submission does not navigate: {typed}"
    );
    // A fifth of the budget (2s), not half: a cancelled submission must stay
    // on the fast post-Enter-sized check, not the wider post-requestSubmit
    // grace period (up to 3s). A 5s (half-budget) bound is too loose to catch
    // that — the review fix that gated the first poll's budget on
    // `load_expected` regressed silently under it until this bound was
    // tightened (2026-09-07).
    assert!(
        elapsed < Duration::from_millis(CLICK_TIMEOUT_MS / 5),
        "a cancelled submission must stay on the fast local check, not the wider \
         post-requestSubmit grace period, took {elapsed:?}"
    );

    stop_daemon(port);
}

// ---------------------------------------------------------------------------
// Part B — the not-found short-circuit, both sides
// ---------------------------------------------------------------------------

/// A page that is complete and then does nothing at all: no timers, no
/// requests, no mutations. Whatever a selector does not match here, it never
/// will.
fn static_fixture() -> HashMap<String, FixtureRoute> {
    let mut routes = HashMap::new();
    routes.insert(
        "/".to_owned(),
        FixtureRoute::html(
            "<!doctype html><title>t237 static</title><body>\
             <h1>Ada Lovelace</h1><button id=\"present\">Present</button></body>",
        ),
    );
    routes
}

/// AC: a selector matching nothing on a settled page is reported in
/// measurably less than `--timeout`.
///
/// The bound is half the budget rather than a tight one: the point of the AC
/// is "not the whole timeout", and a tight bound would turn ordinary CI
/// jitter into a red suite. `main` fails this at ~10 s, not at 5.1 s.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_237_absent_selector_reports_well_under_the_timeout() {
    if !live_tests_enabled() {
        eprintln!(
            "live_237_absent_selector_reports_well_under_the_timeout: set FF_RDP_LIVE_TESTS=1"
        );
        return;
    }
    let ff = firefox_with_daemon("live_237_absent_selector_reports_well_under_the_timeout");
    let port = ff.port();
    let Some(server) = FixtureServer::start(static_fixture()) else {
        eprintln!(
            "live_237_absent_selector_reports_well_under_the_timeout: no fixture HTTP — skipping"
        );
        stop_daemon(port);
        return;
    };

    run_json(port, &["navigate", &server.base_url()]);

    let started = Instant::now();
    let out = run(port, &["click", "a[href=\"/wiki/Charles_Babbage\"]"]);
    let elapsed = started.elapsed();

    assert!(
        !out.status.success(),
        "a selector matching nothing must still fail: stdout={}",
        String::from_utf8_lossy(&out.stdout)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        combined.contains("0 elements matched"),
        "the early answer must keep the not-found diagnostic: {combined}"
    );
    assert!(
        elapsed < Duration::from_millis(CLICK_TIMEOUT_MS / 2),
        "a guessed selector on an idle page must not cost the full \
         {CLICK_TIMEOUT_MS}ms budget, took {elapsed:?}: {combined}"
    );

    stop_daemon(port);
}

/// `/` starts a request shortly after load whose response inserts the button.
///
/// The delay is split on purpose: the `fetch` begins ~600 ms in — after
/// `click`'s auto-wait has installed the settle probe, which is the window in
/// which the counters can see it — and lands ~2.6 s in, past the observation
/// floor a 10 s budget implies. So the element genuinely appears *after* the
/// point where the short-circuit would otherwise have fired, and only the
/// in-flight request keeps the poll alive.
fn late_insert_fixture() -> HashMap<String, FixtureRoute> {
    let mut routes = HashMap::new();
    routes.insert(
        "/".to_owned(),
        FixtureRoute::html(
            "<!doctype html><title>t237 late</title><body><h1>Ada Lovelace</h1>\
             <script>\
             setTimeout(function() {\
               fetch('/late').then(function(r) { return r.text(); }).then(function(t) {\
                 var b = document.createElement('button');\
                 b.id = 'late'; b.textContent = 'Late ' + t.length;\
                 document.body.appendChild(b);\
               });\
             }, 600);\
             </script></body>",
        ),
    );
    routes.insert(
        "/late".to_owned(),
        FixtureRoute::html("<!doctype html><title>t237 late payload</title>ok")
            .with_delay(Duration::from_millis(2_000)),
    );
    routes
}

/// AC (Theme B): the retry case must not regress. A selector that only
/// appears once an in-flight request lands still resolves — the short-circuit
/// is an addition on top of the poll, not a replacement for it.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_237_late_selector_behind_a_request_still_clicks() {
    if !live_tests_enabled() {
        eprintln!("live_237_late_selector_behind_a_request_still_clicks: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_237_late_selector_behind_a_request_still_clicks");
    let port = ff.port();
    let Some(server) = FixtureServer::start(late_insert_fixture()) else {
        eprintln!(
            "live_237_late_selector_behind_a_request_still_clicks: no fixture HTTP — skipping"
        );
        stop_daemon(port);
        return;
    };

    run_json(port, &["navigate", &server.base_url()]);
    let clicked = run_json(port, &["click", "#late"]);

    assert_eq!(
        clicked["results"]["clicked"], true,
        "an element inserted by a pending request must still be waited for: {clicked}"
    );

    stop_daemon(port);
}
