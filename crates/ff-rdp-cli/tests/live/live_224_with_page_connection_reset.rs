//! Live tests for iteration 224 — a connection that dies mid-collection.
//!
//! # The defect
//!
//! After [`super::live_220_navigating_action_with_page`] removed the recv-timeout
//! hang, `click --ref <link> --with-page` on the daemon route still failed
//! intermittently:
//!
//! ```text
//! {"error":"recv failed: Connection reset by peer (os error 54)","error_type":"Transport"}
//! ```
//!
//! exit 6, in ~0.45 s — far too fast to be the settle budget or `--timeout`.
//! Measured on `5a0071d` against `en.wikipedia.org`, daemon route: **2 failures
//! in 30 hops** (`recv failed: Connection reset by peer` once, `recv failed:
//! failed to fill whole buffer` — a FIN mid-frame — once). The click had
//! already been performed; only the view of where it landed was lost, and the
//! agent re-navigated by URL and re-read the page rather than trust it.
//!
//! Two fixes, one on each side of the socket:
//!
//! - the daemon writes a structured `daemon_client_closed` frame before
//!   abandoning a client, instead of dropping the socket silently;
//! - `page_view::collect_settled` treats a lost connection the way it already
//!   treated a destroyed target — rebuild it and collect again inside the
//!   caller's own budget.
//!
//! # What these tests can and cannot prove
//!
//! **They do not reproduce the 1-in-15.** It needs a real remote origin and a
//! real Wikipedia-sized document; against an in-process fixture server the
//! reset does not appear at all (0 failures in 90 local hops during
//! development). What they cover instead is the contract the fix rests on:
//!
//! - the repeated hop returns the destination view every time, and reports what
//!   each view cost (`meta.page_attempts` / `meta.page_reconnects`) — so a
//!   regression that reintroduces the flake shows up as a non-zero reconnect
//!   count in the live sweep rather than as an unexplained exit 6;
//! - no hop ends in a `Transport` / `RemoteClosed` error, which is the exact
//!   failure the iteration exists to remove.
//!
//! The end-to-end evidence for the 1-in-15 itself lives in
//! `kb/iterations/iteration-224-with-page-daemon-connection-reset.md` and in
//! its `.dogfood.sh`, which drives the same hop against the real page.
//!
//! daemon-parity: `--ref` needs the daemon's ref store, so — like
//! [`super::live_220_navigating_action_with_page`] — every test here runs the
//! daemon route.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live live_224_with_page_connection_reset -- --nocapture

use std::collections::HashMap;
use std::fmt::Write as _;
use std::process::{Command, Output};
use std::time::Duration;

use serde_json::Value;

use crate::common::{FixtureRoute, FixtureServer, LiveFirefox, ff_rdp_bin, live_tests_enabled};

/// How many times the hop is repeated.
///
/// The plan asks for N ≥ 10. Twelve at ~1 s each keeps the test inside a live
/// suite's budget while giving the historical 1-in-15 rate a real chance to
/// show itself if it ever becomes reproducible on a local fixture.
const HOPS: usize = 12;

/// How long `/slow` holds the response back before its first byte.
///
/// Short enough that twelve hops stay cheap, long enough that the destination
/// has not committed by the time the collector would naively read the tab —
/// the window iter-220 opened and this test keeps open.
const DESTINATION_DELAY: Duration = Duration::from_millis(250);

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
    let stdout = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
        panic!(
            "output for {args:?} not JSON: {e}\nstdout={stdout}\nstderr={}",
            String::from_utf8_lossy(&out.stderr)
        )
    })
}

/// A destination big enough that its view travels as a chunked LongString —
/// the transfer the reset landed in the middle of.
fn heavy_destination(count: usize) -> String {
    let mut body = String::from(
        "<!doctype html><title>t224 destination</title><body><h1>Charles Babbage</h1>\
         <p>The Analytical Engine was a proposed mechanical general-purpose computer.</p>",
    );
    for i in 0..count {
        let _ = write!(body, "<a href=\"/back\">difference engine {i}</a> ");
    }
    body.push_str("</body>");
    body
}

/// `/` links to `/slow`; `/slow` links back to `/`, so the hop can be repeated
/// without re-navigating by URL between rounds.
fn hop_fixture() -> HashMap<String, FixtureRoute> {
    let mut routes = HashMap::new();
    routes.insert(
        "/".to_owned(),
        FixtureRoute::html(
            "<!doctype html><title>t224 origin</title><body>\
             <h1>Ada Lovelace</h1>\
             <p>Augusta Ada King, Countess of Lovelace, was an English mathematician.</p>\
             <a href=\"/slow\">Charles Babbage</a></body>",
        ),
    );
    routes.insert(
        "/slow".to_owned(),
        FixtureRoute::html(heavy_destination(300)).with_delay(DESTINATION_DELAY),
    );
    routes
}

/// The `ref` of the first interactive entry whose `name` matches, if any.
fn ref_named(page: &Value, name: &str) -> Option<String> {
    page["interactive"]
        .as_array()?
        .iter()
        .find(|e| e["name"] == name)
        .and_then(|e| e["ref"].as_str())
        .map(str::to_owned)
}

fn first_heading(action: &Value) -> Option<&str> {
    action["results"]["page"]["headings"][0]["text"].as_str()
}

// ---------------------------------------------------------------------------
// Theme C — the repeated hop
// ---------------------------------------------------------------------------

/// AC: `HOPS` consecutive `click --ref … --with-page` hops all return the
/// destination view, and none of them ends in a transport-level error.
///
/// The assertion that matters is the *error type*: a `Transport` or
/// `RemoteClosed` failure here is the iter-224 defect, whatever caused it. A
/// per-hop assertion on the heading catches the iter-220 defect (reporting the
/// page you left) recurring on the same path.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_repeated_hop_never_loses_the_connection() {
    if !live_tests_enabled() {
        eprintln!("live_repeated_hop_never_loses_the_connection: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_repeated_hop_never_loses_the_connection");
    let port = ff.port();
    let Some(server) = FixtureServer::start(hop_fixture()) else {
        eprintln!("live_repeated_hop_never_loses_the_connection: no fixture HTTP — skipping");
        stop_daemon(port);
        return;
    };

    let mut transport_failures: Vec<String> = Vec::new();
    let mut reconnects = 0_u64;

    for hop in 1..=HOPS {
        let nav = run_json(port, &["navigate", &server.base_url(), "--with-page"]);
        let Some(ref_id) = ref_named(&nav["results"]["page"], "Charles Babbage") else {
            panic!("hop {hop}: origin page carried no ref for the destination link: {nav}");
        };

        let click = run_json(port, &["click", "--ref", &ref_id, "--with-page"]);

        match click["error_type"].as_str() {
            None => {}
            Some(kind @ ("Transport" | "RemoteClosed")) => {
                transport_failures.push(format!("hop {hop}: {kind}: {}", click["error"]));
                continue;
            }
            Some(other) => panic!("hop {hop}: click failed with {other}: {click}"),
        }

        assert_eq!(
            first_heading(&click),
            Some("Charles Babbage"),
            "hop {hop}: click --with-page must report the destination: {click}"
        );
        reconnects += click["meta"]["page_reconnects"].as_u64().unwrap_or_else(|| {
            panic!("hop {hop}: meta.page_reconnects must always be reported: {click}")
        });
    }

    assert!(
        transport_failures.is_empty(),
        "{} of {HOPS} hops died on the connection — this is the iter-224 defect:\n{}",
        transport_failures.len(),
        transport_failures.join("\n")
    );
    // Not a failure, but worth seeing in `--nocapture`: a non-zero count means
    // the flake reproduced locally *and* the fix absorbed it.
    eprintln!(
        "live_repeated_hop_never_loses_the_connection: {HOPS} hops, {reconnects} reconnect(s) \
         absorbed"
    );

    stop_daemon(port);
}

/// AC: the cost of a view is always reported.
///
/// `meta.page_attempts` / `meta.page_reconnects` are what make a flaky hop
/// visible in JSON at all. They must be present on the clean path too — an
/// absent key would make "collected first try" and "this build predates the
/// counters" indistinguishable, which is the ambiguity iter-224 hit while
/// diagnosing.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_page_view_reports_what_it_cost() {
    if !live_tests_enabled() {
        eprintln!("live_page_view_reports_what_it_cost: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_page_view_reports_what_it_cost");
    let port = ff.port();
    let Some(server) = FixtureServer::start(hop_fixture()) else {
        eprintln!("live_page_view_reports_what_it_cost: no fixture HTTP — skipping");
        stop_daemon(port);
        return;
    };

    let nav = run_json(port, &["navigate", &server.base_url(), "--with-page"]);
    assert_eq!(
        nav["meta"]["page_attempts"].as_u64(),
        Some(1),
        "a plain navigate collects in one attempt: {nav}"
    );
    assert_eq!(
        nav["meta"]["page_reconnects"].as_u64(),
        Some(0),
        "a plain navigate needs no reconnect: {nav}"
    );

    let Some(ref_id) = ref_named(&nav["results"]["page"], "Charles Babbage") else {
        panic!("origin page carried no ref for the destination link: {nav}");
    };
    let click = run_json(port, &["click", "--ref", &ref_id, "--with-page"]);
    assert!(
        click["meta"]["page_attempts"].as_u64().is_some_and(|a| a >= 1),
        "meta.page_attempts must be reported on a navigating click: {click}"
    );
    assert!(
        click["meta"]["page_reconnects"].as_u64().is_some(),
        "meta.page_reconnects must be reported on a navigating click: {click}"
    );

    stop_daemon(port);
}
