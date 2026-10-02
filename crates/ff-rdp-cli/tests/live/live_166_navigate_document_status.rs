//! Live tests for iteration 166 — "`navigate` reports `status: null` for a
//! document it successfully loaded".
//!
//! `navigate` promised the main document's HTTP status from iter-138 Theme A
//! onwards and did not deliver one: measured on `main` at 07a9c03, plain
//! `ff-rdp navigate https://example.com` returned
//! `{"committed_url":"https://example.com/","ready_state":"complete","status":null}`
//! on every connection route AND the `--with-network` route.
//! The cause was an exact-string URL comparison — Firefox canonicalises
//! `https://example.com` to `https://example.com/` before requesting it, so the
//! `cause_type == "document"` resource carrying the 200 never matched.
//!
//! It survived because no test asserted `results.status` on a plain
//! `navigate`: `live_130_navigation_truthfulness` and
//! `live_138_navigation_truthfulness_2` assert `committed_url` and
//! `ready_state` only, and the status field was exercised solely through
//! `--with-network`, whose separate code path carried a separate copy of the
//! same bug. These tests close that gap.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo test-live \
//!       -p ff-rdp-cli --test live live_166 -- --nocapture

use std::collections::HashMap;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

use crate::common::{
    FixtureRoute, FixtureServer, LiveFirefox, ff_rdp_bin, live_network_tests_enabled,
    live_tests_enabled,
};

fn cli_args(port: u16) -> Vec<String> {
    vec![
        "--host".to_owned(),
        "127.0.0.1".to_owned(),
        "--port".to_owned(),
        port.to_string(),
        "--timeout".to_owned(),
        "30000".to_owned(),
    ]
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
    if std::env::var_os("FF_RDP_277_CAPTURE").is_some() {
        eprintln!(
            "277 capture global={global:?} navigate={args:?} status={}\n{}",
            out.status,
            combined(&out)
        );
    }
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
// Why these tests fetch a *fresh* example.com URL every time (iteration 235
// Part B — is 304 acceptable here?)
// ---------------------------------------------------------------------------

/// Serial number making each `example.com` URL below unique within a process.
static CACHE_BUSTER: AtomicU64 = AtomicU64::new(0);

/// A `https://example.com` URL Firefox has never fetched, returned as
/// `(url_as_typed, url_after_canonicalisation)`.
///
/// # Why the cache buster
///
/// `example.com` answers with `Cache-Control: max-age=604800`, so the *second*
/// and later navigations to it inside one profile are conditional requests and
/// the origin answers `304 Not Modified`. These two tests asserted a flat
/// `200` and so were red on any warm profile — twice in production sweeps
/// (iteration 210, 2026-08-24; iteration 220, 2026-08-30), each time costing
/// an investigation that concluded the same thing.
///
/// **The 304 is ff-rdp being right, and the assertion was wrong.**
/// `results.status` reporting the document's real 304 is precisely the
/// document-status truthfulness iteration 166 was built to deliver; the test
/// encoded "the first, uncached fetch" as if it were "any fetch".
///
/// So 304 is *not* accepted here — it is *prevented*. A unique query string
/// gives every navigation its own HTTP cache key, making the load
/// unconditional, so the strict `200` keeps its original meaning: **the server
/// answered 200**. The alternative — widening the assertion to "200 or 304" —
/// was rejected deliberately: it would then also pass if ff-rdp reported 304
/// for a navigation that genuinely got 200, which is exactly the class of
/// defect iteration 166 exists to catch.
///
/// The query string does not weaken the canonicalisation coverage that is the
/// point of the no-trailing-slash leg: Firefox still rewrites
/// `https://example.com?x` to `https://example.com/?x`, the missing-slash shape
/// that *was* the iteration 166 defect.
fn uncached_example_url(path: &str) -> (String, String) {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let serial = CACHE_BUSTER.fetch_add(1, Ordering::Relaxed);
    let query = format!("?ff-rdp-cache-bust={nanos}-{serial}");
    (
        format!("https://example.com{path}{query}"),
        format!("https://example.com/{query}"),
    )
}

// ---------------------------------------------------------------------------
// AC live_166_navigate_reports_document_status
// ---------------------------------------------------------------------------

/// AC: `live_166_navigate_reports_document_status`.
///
/// The exact `dogfood_path` repro. Measured on main
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
    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();
    let global = cli_args(port);

    // No trailing slash — the form the plan's dogfood_path uses and the form
    // that reported `null` on main. Each leg gets its own cache-busting query
    // so the fetch is unconditional and `200` stays the honest expectation;
    // see `uncached_example_url`.
    let (url, canonical) = uncached_example_url("");
    let results = navigate_ok(&global, &[&url], "plain");
    assert_eq!(
        results["committed_url"], canonical,
        "sanity: the navigation itself must have succeeded, got {results}"
    );
    assert_eq!(results["ready_state"], "complete");
    assert_status(&results, 200, "no trailing slash");

    // The canonical form must agree — it worked on main and must not regress.
    let (url, _) = uncached_example_url("/");
    let results = navigate_ok(&global, &[&url], "plain");
    assert_status(&results, 200, "trailing slash");

    // `--with-network` reaches the status through an entirely separate code
    // path. It reported `null` on main too, and must now agree.
    let (url, _) = uncached_example_url("");
    let results = navigate_ok(&global, &[&url, "--with-network"], "--with-network");
    assert_status(&results, 200, "--with-network");
}

// ---------------------------------------------------------------------------
// The status is the server's, not a constant
// ---------------------------------------------------------------------------

/// A 200 that is always 200 proves nothing. Against a local fixture server
/// (no network gate needed), a served route reports 200 and an unknown path
/// reports the server's 404.
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
    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();

    {
        let (label, global) = ("plain", cli_args(port));
        let ok_url = format!("{base}/ok");
        let results = navigate_ok(&global, &[&ok_url], label);
        assert_status(&results, 200, &format!("{label} /ok"));

        let missing_url = format!("{base}/definitely-not-here");
        let results = navigate_ok(&global, &[&missing_url], label);
        assert_status(&results, 404, &format!("{label} /definitely-not-here"));
    }
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
    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();
    let global = cli_args(port);

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
}
