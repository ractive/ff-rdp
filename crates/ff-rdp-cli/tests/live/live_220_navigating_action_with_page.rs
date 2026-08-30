//! Live tests for iteration 220 — `--with-page` after an action that navigates.
//!
//! `click --ref <link> --with-page` on `en.wikipedia.org/wiki/Ada_Lovelace`
//! timed out at `phase: recv` for the whole `--timeout` budget, 3 runs out of
//! 3, from iter-210 until iter-220. Three live suites and two iterations
//! shipped over it because every fixture they used is a two-element page served
//! from an in-process HTTP server: it commits before a collector can race it,
//! so the window in which `getTarget` still hands back the *outgoing* docshell
//! never opens.
//!
//! These tests open that window on purpose. `/slow` sits on the request for
//! [`DESTINATION_DELAY`] before its first byte and then serves several hundred
//! interactive elements, so between the click and the commit there is a wide,
//! reproducible interval during which the naive implementation collects the
//! page the action *left*.
//!
//! What that buys is a deterministic failure on `main` rather than a flaky one:
//! the pre-fix code does not usually hang here (the origin docshell survives
//! long enough to answer) — it succeeds and reports the ORIGIN page's `<h1>`.
//! Same defect, same fix, no timing lottery in CI.
//!
//! The last two tests are the other half of the contract: waiting for a
//! navigation must cost nothing when there is no navigation to wait for. A
//! click on a button and a click on a `#fragment` link both have to come back
//! promptly, not after the settle budget.
//!
//! daemon-parity: `--ref` needs the daemon's ref store, so — like
//! [`super::live_210_act_and_see`] — every test here runs the daemon route.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live live_220_navigating_action_with_page -- --nocapture

use std::collections::HashMap;
use std::fmt::Write as _;
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::common::{FixtureRoute, FixtureServer, LiveFirefox, ff_rdp_bin, live_tests_enabled};

/// How long `/slow` holds the response back before its first byte.
///
/// Long enough that the collector cannot possibly beat the commit by luck,
/// short enough that four tests paying it twice each stay comfortable inside a
/// live-suite budget.
const DESTINATION_DELAY: Duration = Duration::from_millis(700);

/// Upper bound on a `--with-page` call that has NO navigation to wait for.
///
/// `page_view::NAV_SETTLE_BUDGET_MS` is 3 s. A non-navigating click must not
/// come anywhere near it: this bound fails if the settle loop ever runs on a
/// click that did not navigate.
const NO_NAVIGATION_BUDGET: Duration = Duration::from_millis(2_500);

fn daemon_args(port: u16) -> Vec<String> {
    vec![
        "--host".to_owned(),
        "127.0.0.1".to_owned(),
        "--port".to_owned(),
        port.to_string(),
        "--timeout".to_owned(),
        "20000".to_owned(),
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

/// The `/slow` body: one `<h1>` plus `count` links, so the collected view is
/// large enough to travel as a chunked LongString — the code path that hung
/// on the direct (`--no-daemon`) route while the daemon route hung one step
/// earlier, in the eval itself.
fn heavy_destination(count: usize) -> String {
    let mut body = String::from(
        "<!doctype html><title>t220 destination</title><body><h1>Charles Babbage</h1>",
    );
    for i in 0..count {
        let _ = write!(body, "<a href=\"/other#{i}\">difference engine {i}</a> ");
    }
    body.push_str("</body>");
    body
}

/// `/` links (and submits) to `/slow`, which is slow to commit and heavy.
///
/// `/` also carries a button and a same-document `#here` link so the
/// "no navigation to wait for" tests can share the fixture.
fn slow_link_fixture() -> HashMap<String, FixtureRoute> {
    let mut routes = HashMap::new();
    routes.insert(
        "/".to_owned(),
        FixtureRoute::html(
            "<!doctype html><title>t220 origin</title><body>\
             <h1>Ada Lovelace</h1>\
             <a href=\"/slow\">Charles Babbage</a>\
             <form action=\"/slow\" method=\"get\">\
             <input name=\"q\" aria-label=\"Search\">\
             </form>\
             <button id=\"noop\" onclick=\"void 0\">Ignore me</button>\
             <a id=\"frag\" href=\"#here\">Jump down</a>\
             <p id=\"here\">anchor</p></body>",
        ),
    );
    routes.insert(
        "/slow".to_owned(),
        FixtureRoute::html(heavy_destination(400)).with_delay(DESTINATION_DELAY),
    );
    routes
}

/// The `ref` of the first interactive entry whose `name` matches.
fn ref_named(page: &Value, name: &str) -> String {
    page["interactive"]
        .as_array()
        .unwrap_or_else(|| panic!("page.interactive must be an array: {page}"))
        .iter()
        .find(|e| e["name"] == name)
        .and_then(|e| e["ref"].as_str())
        .unwrap_or_else(|| panic!("no ref for interactive entry named {name:?} in {page}"))
        .to_owned()
}

fn first_heading(action: &Value) -> String {
    action["results"]["page"]["headings"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("results.page.headings[0].text missing: {action}"))
        .to_owned()
}

// ---------------------------------------------------------------------------
// Theme A/B — the defect
// ---------------------------------------------------------------------------

/// AC: `click --ref <link> --with-page` returns the DESTINATION's view when the
/// destination is slow to commit.
///
/// Fails on `main`: the collector refreshes the target, gets the outgoing
/// document back (same `innerWindowId`, same URL, merely re-forwarded under a
/// new `childN/` prefix), and reports `Ada Lovelace`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_click_with_page_waits_for_slow_destination() {
    if !live_tests_enabled() {
        eprintln!("live_click_with_page_waits_for_slow_destination: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_click_with_page_waits_for_slow_destination");
    let port = ff.port();
    let Some(server) = FixtureServer::start(slow_link_fixture()) else {
        eprintln!("live_click_with_page_waits_for_slow_destination: no fixture HTTP — skipping");
        stop_daemon(port);
        return;
    };

    let nav = run_json(port, &["navigate", &server.base_url(), "--with-page"]);
    assert_eq!(
        first_heading(&nav),
        "Ada Lovelace",
        "the origin page must be the fixture's `/`: {nav}"
    );
    let ref_id = ref_named(&nav["results"]["page"], "Charles Babbage");

    let click = run_json(port, &["click", "--ref", &ref_id, "--with-page"]);
    assert_eq!(
        first_heading(&click),
        "Charles Babbage",
        "click --with-page must report the destination it navigated to, not the page it left: {click}"
    );

    stop_daemon(port);
}

/// AC: `type --submit --with-page` takes the identical path and must behave
/// identically.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_type_submit_with_page_waits_for_slow_destination() {
    if !live_tests_enabled() {
        eprintln!("live_type_submit_with_page_waits_for_slow_destination: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_type_submit_with_page_waits_for_slow_destination");
    let port = ff.port();
    let Some(server) = FixtureServer::start(slow_link_fixture()) else {
        eprintln!(
            "live_type_submit_with_page_waits_for_slow_destination: no fixture HTTP — skipping"
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
        "type --submit --with-page must report the page the submission produced: {typed}"
    );

    stop_daemon(port);
}

/// The destination view must actually be the heavy one — several hundred
/// interactive entries behind the cap — or the test above would pass on a
/// destination that never exercised the long-string path the direct route hung
/// in.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_slow_destination_view_is_the_heavy_one() {
    if !live_tests_enabled() {
        eprintln!("live_slow_destination_view_is_the_heavy_one: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_slow_destination_view_is_the_heavy_one");
    let port = ff.port();
    let Some(server) = FixtureServer::start(slow_link_fixture()) else {
        eprintln!("live_slow_destination_view_is_the_heavy_one: no fixture HTTP — skipping");
        stop_daemon(port);
        return;
    };

    run_json(port, &["navigate", &server.base_url()]);
    let click = run_json(port, &["click", "a[href='/slow']", "--with-page"]);
    let total = click["results"]["page"]["interactive_total"]
        .as_u64()
        .unwrap_or_else(|| panic!("page.interactive_total missing: {click}"));
    assert!(
        total >= 400,
        "the destination must be link-heavy enough to exercise the chunked \
         long-string path — interactive_total={total}: {click}"
    );

    stop_daemon(port);
}

// ---------------------------------------------------------------------------
// The cost side — waiting must be free when there is nothing to wait for
// ---------------------------------------------------------------------------

/// A click that does NOT navigate must not pay the settle budget.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_non_navigating_click_with_page_is_not_delayed() {
    if !live_tests_enabled() {
        eprintln!("live_non_navigating_click_with_page_is_not_delayed: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_non_navigating_click_with_page_is_not_delayed");
    let port = ff.port();
    let Some(server) = FixtureServer::start(slow_link_fixture()) else {
        eprintln!("live_non_navigating_click_with_page_is_not_delayed: no fixture HTTP — skipping");
        stop_daemon(port);
        return;
    };

    run_json(port, &["navigate", &server.base_url()]);
    let started = Instant::now();
    let click = run_json(port, &["click", "#noop", "--with-page"]);
    let elapsed = started.elapsed();

    assert_eq!(
        first_heading(&click),
        "Ada Lovelace",
        "a click that navigates nowhere must report the page it is still on: {click}"
    );
    assert!(
        elapsed < NO_NAVIGATION_BUDGET,
        "a non-navigating click --with-page took {elapsed:?}, over the {NO_NAVIGATION_BUDGET:?} \
         bound — the navigation settle loop must not run when nothing navigated: {click}"
    );

    stop_daemon(port);
}

/// A same-document `#fragment` click announces a navigation but never changes
/// `innerWindowId`, so waiting on the id alone would burn the whole budget. The
/// settle loop's URL exit is what keeps this fast.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_fragment_click_with_page_is_not_delayed() {
    if !live_tests_enabled() {
        eprintln!("live_fragment_click_with_page_is_not_delayed: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_fragment_click_with_page_is_not_delayed");
    let port = ff.port();
    let Some(server) = FixtureServer::start(slow_link_fixture()) else {
        eprintln!("live_fragment_click_with_page_is_not_delayed: no fixture HTTP — skipping");
        stop_daemon(port);
        return;
    };

    run_json(port, &["navigate", &server.base_url()]);
    let started = Instant::now();
    let click = run_json(port, &["click", "#frag", "--with-page"]);
    let elapsed = started.elapsed();

    assert_eq!(
        first_heading(&click),
        "Ada Lovelace",
        "a same-document fragment click stays on the same document: {click}"
    );
    assert!(
        elapsed < NO_NAVIGATION_BUDGET,
        "a fragment click --with-page took {elapsed:?}, over the {NO_NAVIGATION_BUDGET:?} bound \
         — the settle loop must exit on the URL match instead of waiting for an \
         innerWindowId that never changes: {click}"
    );

    stop_daemon(port);
}
