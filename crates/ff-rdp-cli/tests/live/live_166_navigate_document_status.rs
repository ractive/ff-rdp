//! Live tests for iteration 166 — "`navigate` reports `status: null` for a
//! document it successfully loaded".
//!
//! `navigate` promised the main document's HTTP status from iter-138 Theme A
//! onwards and did not deliver one: measured on `main` at 07a9c03, plain
//! `ff-rdp navigate https://example.com` returned
//! `{"committed_url":"https://example.com/","ready_state":"complete","status":null}`
//! on the daemon route, the `--no-daemon` route AND the `--with-network` route.
//! The cause was an exact-string URL comparison — Firefox canonicalises
//! `https://example.com` to `https://example.com/` before requesting it, so the
//! `cause_type == "document"` resource carrying the 200 never matched.
//!
//! It survived because no test asserted `results.status` on a plain
//! `navigate`: `live_130_navigation_truthfulness` and
//! `live_138_navigation_truthfulness_2` assert `committed_url` and
//! `ready_state` only, and the status field was exercised solely through
//! `--with-network`, whose separate code path carried a separate copy of the
//! same bug. These tests close that gap on **both** connection routes, per
//! CONTRIBUTING's daemon-parity rule.
//!
//! daemon-parity: `live_166_navigate_reports_document_status` is the daemon
//! leg (the mode every real invocation uses) and
//! `live_166_navigate_status_direct_parity` is the `--no-daemon` leg, so the
//! two cannot diverge again unnoticed.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo test-live \
//!       -p ff-rdp-cli --test live live_166 -- --nocapture

use std::collections::HashMap;
use std::process::{Command, Output};

use serde_json::Value;

use crate::common::{
    FixtureRoute, FixtureServer, LiveFirefox, ff_rdp_bin, live_network_tests_enabled,
    live_tests_enabled,
};

/// Global args for the **default** connection mode: no `--no-daemon`, so the
/// CLI auto-starts and proxies through the daemon.
fn daemon_args(port: u16) -> Vec<String> {
    vec![
        "--host".to_owned(),
        "127.0.0.1".to_owned(),
        "--port".to_owned(),
        port.to_string(),
        "--timeout".to_owned(),
        "30000".to_owned(),
    ]
}

/// Global args for a direct connection — the other half of daemon parity.
fn direct_args(port: u16) -> Vec<String> {
    let mut args = daemon_args(port);
    args.push("--no-daemon".to_owned());
    args
}

fn stop_daemon(port: u16) {
    let _ = Command::new(ff_rdp_bin())
        .args(["--host", "127.0.0.1", "--port", &port.to_string()])
        .args(["daemon", "stop"])
        .output();
}

/// Bring up Firefox with a running daemon, panicking on failure (iter-158
/// Theme D: an `Option` here made every caller `return`, which libtest
/// reports as `ok`).
fn firefox_with_daemon(test: &str) -> LiveFirefox {
    let ff = LiveFirefox::headless_on_random_port();
    assert!(
        ff.with_daemon().is_some(),
        "{test}: the proxy daemon did not start for Firefox on port {}",
        ff.port()
    );
    ff
}

fn combined(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// Run `ff-rdp <global> navigate <args>` and return its `results` object,
/// asserting exit 0.
fn navigate_ok(global: &[String], args: &[&str], label: &str) -> Value {
    let out = Command::new(ff_rdp_bin())
        .args(global)
        .arg("navigate")
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("{label}: spawn ff-rdp navigate {args:?}: {e}"));
    assert!(
        out.status.success(),
        "{label}: `navigate {args:?}` must exit 0; got: {}",
        combined(&out)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("{label}: output not JSON: {e}\n{stdout}"));
    v["results"].clone()
}

/// Assert the envelope's status/`status_reason` invariant: exactly one of the
/// two is non-`null`, both keys are always present, and — when a status was
/// observed — it is the one expected.
fn assert_status(results: &Value, want: u64, label: &str) {
    assert!(
        results.get("status").is_some(),
        "{label}: `status` must always be present, got {results}"
    );
    assert!(
        results.get("status_reason").is_some(),
        "{label}: `status_reason` must always be present, got {results}"
    );
    assert_eq!(
        results["status"],
        Value::from(want),
        "{label}: expected HTTP {want}, got {results}"
    );
    assert_eq!(
        results["status_reason"],
        Value::Null,
        "{label}: `status_reason` must be null when a status was observed, got {results}"
    );
}

/// Two routes on a local fixture server: `/ok` returns 200, and any unknown
/// path returns 404 — so a test can prove the reported status tracks the
/// server rather than being a hardcoded 200.
fn fixture_routes() -> HashMap<String, FixtureRoute> {
    let mut routes = HashMap::new();
    routes.insert(
        "/ok".to_owned(),
        FixtureRoute::html("<html><body><h1>ok</h1></body></html>"),
    );
    routes
}

// ---------------------------------------------------------------------------
// AC live_166_navigate_reports_document_status
// ---------------------------------------------------------------------------

/// AC: `live_166_navigate_reports_document_status`.
///
/// The exact `dogfood_path` repro over the **daemon** route. Measured on main
/// before the fix: `status: null`. It must be 200 — and it must be 200 for the
/// URL as a caller actually types it, without the trailing slash Firefox adds,
/// because that missing slash *was* the defect.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_166_navigate_reports_document_status() {
    if !live_tests_enabled() {
        eprintln!("live_166_navigate_reports_document_status: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    if !live_network_tests_enabled() {
        eprintln!(
            "live_166_navigate_reports_document_status: set FF_RDP_LIVE_NETWORK_TESTS=1 \
             (this test fetches https://example.com, the plan's dogfood_path)"
        );
        return;
    }
    let ff = firefox_with_daemon("live_166_navigate_reports_document_status");
    let port = ff.port();
    let global = daemon_args(port);

    // No trailing slash — the form the plan's dogfood_path uses and the form
    // that reported `null` on main.
    let results = navigate_ok(&global, &["https://example.com"], "daemon");
    assert_eq!(
        results["committed_url"], "https://example.com/",
        "sanity: the navigation itself must have succeeded, got {results}"
    );
    assert_eq!(results["ready_state"], "complete");
    assert_status(&results, 200, "daemon, no trailing slash");

    // The canonical form must agree — it worked on main and must not regress.
    let results = navigate_ok(&global, &["https://example.com/"], "daemon");
    assert_status(&results, 200, "daemon, trailing slash");

    // `--with-network` reaches the status through an entirely separate code
    // path (it drains the daemon buffer rather than correlating a streamed
    // event). It reported `null` on main too, and must now agree.
    let results = navigate_ok(
        &global,
        &["https://example.com", "--with-network"],
        "daemon --with-network",
    );
    assert_status(&results, 200, "daemon --with-network");

    stop_daemon(port);
}

// ---------------------------------------------------------------------------
// AC live_166_navigate_status_direct_parity
// ---------------------------------------------------------------------------

/// AC: `live_166_navigate_status_direct_parity`.
///
/// The same assertion over `--no-daemon`. The plan's Theme A expected the
/// defect to be daemon-specific; the measurement disproved that — direct mode
/// reported `null` too — so this leg is not a formality, it is half the bug.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_166_navigate_status_direct_parity() {
    if !live_tests_enabled() {
        eprintln!("live_166_navigate_status_direct_parity: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    if !live_network_tests_enabled() {
        eprintln!(
            "live_166_navigate_status_direct_parity: set FF_RDP_LIVE_NETWORK_TESTS=1 \
             (this test fetches https://example.com, the plan's dogfood_path)"
        );
        return;
    }
    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();
    let global = direct_args(port);

    let results = navigate_ok(&global, &["https://example.com"], "direct");
    assert_eq!(
        results["committed_url"], "https://example.com/",
        "sanity: the navigation itself must have succeeded, got {results}"
    );
    assert_status(&results, 200, "--no-daemon, no trailing slash");

    let results = navigate_ok(
        &global,
        &["https://example.com", "--with-network"],
        "direct --with-network",
    );
    assert_status(&results, 200, "--no-daemon --with-network");
}

// ---------------------------------------------------------------------------
// The status is the server's, not a constant
// ---------------------------------------------------------------------------

/// A 200 that is always 200 proves nothing. Against a local fixture server
/// (no network gate needed), a served route reports 200 and an unknown path
/// reports the server's 404 — on both routes.
///
/// The fixture server's base URL has no path at all
/// (`http://127.0.0.1:<port>/missing` does, but `…:<port>` alone does not),
/// which is the same canonicalisation shape as the `example.com` repro.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_166_navigate_status_reflects_the_server() {
    if !live_tests_enabled() {
        eprintln!("live_166_navigate_status_reflects_the_server: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let Some(server) = FixtureServer::start(fixture_routes()) else {
        panic!("live_166_navigate_status_reflects_the_server: could not bind a fixture server");
    };
    let base = server.base_url();
    let ff = firefox_with_daemon("live_166_navigate_status_reflects_the_server");
    let port = ff.port();

    for (label, global) in [("daemon", daemon_args(port)), ("direct", direct_args(port))] {
        let ok_url = format!("{base}/ok");
        let results = navigate_ok(&global, &[&ok_url], label);
        assert_status(&results, 200, &format!("{label} /ok"));

        let missing_url = format!("{base}/definitely-not-here");
        let results = navigate_ok(&global, &[&missing_url], label);
        assert_status(&results, 404, &format!("{label} /definitely-not-here"));
    }

    stop_daemon(port);
}

// ---------------------------------------------------------------------------
// `null` now means something
// ---------------------------------------------------------------------------

/// Theme B: a `null` status always arrives with a `status_reason` naming which
/// of the three situations produced it, so a caller can tell "the server sent
/// no status" from "this route never looked".
///
/// `about:blank` commits without issuing any request at all
/// (`no_document_request`), and `--no-wait` never subscribes to `network-event`
/// in the first place (`not_observed`). Both reported an indistinguishable bare
/// `null` before iter-166.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_166_null_status_carries_a_reason() {
    if !live_tests_enabled() {
        eprintln!("live_166_null_status_carries_a_reason: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let Some(server) = FixtureServer::start(fixture_routes()) else {
        panic!("live_166_null_status_carries_a_reason: could not bind a fixture server");
    };
    let base = server.base_url();
    let ff = firefox_with_daemon("live_166_null_status_carries_a_reason");
    let port = ff.port();
    let global = daemon_args(port);

    // A document that issues no network request of its own.
    let results = navigate_ok(
        &global,
        &["about:blank", "--allow-unsafe-urls"],
        "about:blank",
    );
    assert_eq!(
        results["status"],
        Value::Null,
        "about:blank has no HTTP status, got {results}"
    );
    assert_eq!(
        results["status_reason"], "no_document_request",
        "a bare `null` must say why, got {results}"
    );

    // `--no-wait` returns before any subscription exists, so nothing was ever
    // observed — a different `null` from the one above.
    let ok_url = format!("{base}/ok");
    let results = navigate_ok(&global, &[&ok_url, "--no-wait"], "--no-wait");
    assert_eq!(results["status"], Value::Null);
    assert_eq!(
        results["status_reason"], "not_observed",
        "`--no-wait` never subscribes, so it must not imply the server was \
         silent, got {results}"
    );

    stop_daemon(port);
}
