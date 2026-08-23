//! Live tests for iteration 174 — on a **direct** connection the navigation
//! wait never received `dom-loading`/`dom-interactive`/`dom-complete`, so every
//! `reload`/`back`/`forward` (and `navigate --wait-strategy events`) burned its
//! whole events budget before the `document.readyState` fallback answered.
//!
//! Cause, measured on FF154 against a static localhost page: the direct route's
//! `getWatcher` omitted `isServerTargetSwitchingEnabled`. Without it Firefox
//! never instantiates a watcher-owned target for the top-level window global,
//! so the content-process `document-event` watcher that emits the three `dom-*`
//! events never runs. `watchTargets("frame")` and `watchResources` are both
//! accepted and acked, and the *parent*-process resources (`will-navigate`,
//! `network-event`) keep arriving — which is why this looked like a working
//! connection for four iterations. The daemon's `establish_watcher` always
//! passed the flag, hence the ~190x split between the two routes:
//!
//! ```text
//! ff-rdp --no-daemon --timeout 30000 reload   → elapsed_ms 21011   (before)
//! ff-rdp --no-daemon --timeout 30000 reload   → elapsed_ms   115   (after)
//! ff-rdp --timeout 30000 reload  (daemon)     → elapsed_ms   111   (both)
//! ```
//!
//! Why the defect survived four iterations: `live_130_reload_envelope` and
//! `live_138_back_forward_committed_url_is_top_frame` assert `committed_url` /
//! `ready_state` / the mere *presence* of `elapsed_ms` — every one of which the
//! readystate fallback supplies. Nothing bounded `elapsed_ms`, and nothing
//! asserted that the events path was the one that answered. Both gaps are
//! closed here: `elapsed_ms` is bounded, and `status` is asserted, which the
//! fallback path structurally cannot produce (it correlates no document
//! request, so it reports `status_reason: "not_observed"`).
//!
//! A second defect fell out of the fix and is fixed here too: with the events
//! path working, a bad-DNS `navigate` no longer times out, so the neterror
//! reclassification that only ran on `AppError::Timeout` stopped firing.
//! Firefox reports the **failed** URL from `location.href` *and* from every
//! `document-event` (measured: `dom-loading` url =
//! `https://…invalid/`, never `about:neterror`), so only `listTabs` can tell
//! the two apart. `run_core` now runs that check on the success path when no
//! HTTP status was observed. The daemon route had been returning `exit 0` with
//! a success envelope for a DNS failure all along — `live_61l::live_navigate_dnsfail`
//! is direct-only and never looked.
//!
//! daemon-parity: every assertion below runs on both routes —
//! `live_174_nav_verbs_resolve_from_events_direct` is the `--no-daemon` leg and
//! `live_174_nav_verbs_resolve_from_events_daemon` the proxied one, because the
//! bug was precisely a divergence between them.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli --test live live_174 -- --nocapture

use std::collections::HashMap;
use std::process::{Command, Output};

use serde_json::Value;

use crate::common::{FixtureRoute, FixtureServer, LiveFirefox, ff_rdp_bin, live_tests_enabled};

/// The bound iteration 174's AC states: a navigation verb on a static
/// localhost page must resolve from the events path, and the events path
/// answers in ~100 ms. The pre-fix readystate fallback answered at 21 011 ms
/// (`split_wait_budget(30_000).1` burnt in full, plus the poll). 2 000 ms sits
/// an order of magnitude below that and an order of magnitude above a healthy
/// run, so it fails on the defect without flaking on a loaded CI box.
const MAX_COMMIT_MS: u64 = 2_000;

/// `--timeout` used for every invocation. Large on purpose: the failure mode
/// this test guards against is *proportional to the timeout*, so a generous
/// value makes the regression unmistakable (21 s, not 0.7 s) rather than
/// hiding it.
const TIMEOUT_MS: &str = "30000";

/// A host that cannot resolve, for the neterror leg. `.invalid` is reserved by
/// RFC 2606 precisely so it never resolves.
const BAD_HOST: &str = "https://this-domain-totally-does-not-exist-174-zzz.invalid";

fn daemon_args(port: u16) -> Vec<String> {
    vec![
        "--host".to_owned(),
        "127.0.0.1".to_owned(),
        "--port".to_owned(),
        port.to_string(),
        "--timeout".to_owned(),
        TIMEOUT_MS.to_owned(),
    ]
}

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

fn combined(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// Run `ff-rdp <global> <args…>`, assert exit 0, and return `results`.
///
/// The exit-status assertion is itself part of AC 3: before this iteration a
/// `back`/`forward` on a direct connection could exhaust the readystate
/// fallback too and exit 124.
fn run_ok(global: &[String], args: &[&str], label: &str) -> Value {
    let out = Command::new(ff_rdp_bin())
        .args(global)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("{label}: spawn ff-rdp {args:?}: {e}"));
    assert!(
        out.status.success(),
        "{label}: `{args:?}` must exit 0 (exit 124 = the events budget and the \
         readystate fallback were both exhausted — iteration 174's defect); got: {}",
        combined(&out)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("{label}: output not JSON: {e}\n{stdout}"));
    v["results"].clone()
}

/// Assert the commit was answered by the **events** path, not the readystate
/// fallback.
///
/// Two independent signals, because either alone is weak:
///
/// * `elapsed_ms` under [`MAX_COMMIT_MS`] — the fallback cannot be that fast,
///   it only runs after the events budget is spent.
/// * `status` present — the fallback correlates no `network-event` for the
///   document, so it structurally reports `status_reason: "not_observed"`.
fn assert_resolved_from_events(results: &Value, label: &str) {
    let elapsed = results["elapsed_ms"]
        .as_u64()
        .unwrap_or_else(|| panic!("{label}: `elapsed_ms` must be a number, got {results}"));
    assert!(
        elapsed < MAX_COMMIT_MS,
        "{label}: elapsed_ms {elapsed} >= {MAX_COMMIT_MS} — the events wait was \
         starved and the readystate fallback answered (iteration 174 measured \
         21011 ms here). Full envelope: {results}"
    );
    assert_ne!(
        results["status_reason"], "not_observed",
        "{label}: `not_observed` means no document request was ever correlated, \
         i.e. the readystate fallback produced this envelope, not the events \
         path. Full envelope: {results}"
    );
}

fn fixture_routes() -> HashMap<String, FixtureRoute> {
    let mut routes = HashMap::new();
    routes.insert(
        "/a".to_owned(),
        FixtureRoute::html("<!doctype html><title>iter-174 A</title><h1>A</h1>"),
    );
    routes.insert(
        "/b".to_owned(),
        FixtureRoute::html("<!doctype html><title>iter-174 B</title><h1>B</h1>"),
    );
    routes
}

/// Exercise the four navigation verbs on one Firefox over one connection mode.
fn exercise_events_path(global: &[String], route: &str) {
    let Some(server) = FixtureServer::start(fixture_routes()) else {
        panic!("{route}: could not bind the local fixture server");
    };
    let url_a = format!("{}/a", server.base_url());
    let url_b = format!("{}/b", server.base_url());

    run_ok(global, &["navigate", &url_a], &format!("{route}: nav a"));

    // --- navigate --wait-strategy events ---------------------------------
    // The one-command check iteration 174's plan called for first: this
    // strategy has no readystate fallback at all, so before the fix it did not
    // merely run slow on a direct connection — it timed out unconditionally,
    // proving the defect sat in the shared `getWatcher`/`watchTargets` prelude
    // and covered all four verbs rather than the three history ones.
    let label = format!("{route}: navigate --wait-strategy events");
    let results = run_ok(
        global,
        &["navigate", &url_b, "--wait-strategy", "events"],
        &label,
    );
    assert_resolved_from_events(&results, &label);

    // --- reload -----------------------------------------------------------
    // AC 2: `elapsed_ms` under 2 000 ms against the 21 029 ms the plan
    // measured. AC 4's `status == 200` assertion lives in
    // `live_169_nav_verb_status_parity`, which now runs on both routes.
    let label = format!("{route}: reload");
    let results = run_ok(global, &["reload"], &label);
    assert_resolved_from_events(&results, &label);

    // --- back / forward ---------------------------------------------------
    // AC 3: exit 0 (asserted by `run_ok`) and a bounded commit. `status` is
    // deliberately NOT asserted: a traversal served from BFCache issues no
    // request, so `no_document_request` is the honest answer there — but it is
    // a *different* reason from `not_observed`, which is what
    // `assert_resolved_from_events` rejects.
    let label = format!("{route}: back");
    let results = run_ok(global, &["back"], &label);
    assert_eq!(
        results["committed_url"], url_a,
        "{label}: back must land on {url_a}, got {results}"
    );
    assert_resolved_from_events(&results, &label);

    let label = format!("{route}: forward");
    let results = run_ok(global, &["forward"], &label);
    assert_eq!(
        results["committed_url"], url_b,
        "{label}: forward must land on {url_b}, got {results}"
    );
    assert_resolved_from_events(&results, &label);
}

/// A DNS failure must exit 7 (`nav_dns_fail`) on **both** routes.
///
/// Direct: this is a regression guard for the fix — before iteration 174 it
/// passed only because the events wait timed out and
/// `reclassify_timeout_as_neterror` caught it on the way out.
/// Daemon: this never passed before iteration 174. `navigate` to a
/// non-resolving host returned `exit 0` with
/// `{"committed_url": "https://…invalid/", "ready_state": "complete"}` — a
/// success envelope for a page that never loaded.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_174_dns_failure_exits_nav_dns_fail_both_routes() {
    if !live_tests_enabled() {
        eprintln!("live_174_dns_failure_exits_nav_dns_fail_both_routes: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    if std::env::var("FF_RDP_LIVE_NETWORK_TESTS").is_err() {
        eprintln!(
            "live_174_dns_failure_exits_nav_dns_fail_both_routes: requires real DNS; set \
             FF_RDP_LIVE_NETWORK_TESTS=1 to run"
        );
        return;
    }
    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();
    assert!(
        ff.with_daemon().is_some(),
        "live_174_dns_failure_exits_nav_dns_fail_both_routes: the proxy daemon did \
         not start for Firefox on port {port}"
    );

    for (route, global) in [("direct", direct_args(port)), ("daemon", daemon_args(port))] {
        let out = Command::new(ff_rdp_bin())
            .args(&global)
            .args(["navigate", BAD_HOST])
            .output()
            .unwrap_or_else(|e| panic!("{route}: spawn ff-rdp navigate {BAD_HOST}: {e}"));
        let all = combined(&out);
        assert!(
            !out.status.success(),
            "{route}: navigate to a non-resolving host must not exit 0 — a success \
             envelope here is a page that never loaded being reported as loaded. Got: {all}"
        );
        assert!(
            all.contains("nav_dns_fail"),
            "{route}: the failure must be classified, not a bare timeout. Got: {all}"
        );
    }
    stop_daemon(port);
}

/// AC: "`ff-rdp --no-daemon --timeout 30000 reload` on a static localhost page
/// reports `elapsed_ms` under 2 000 ms" and "`back`/`forward` exit 0 on a page
/// with history" — the leg that reproduces the defect.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_174_nav_verbs_resolve_from_events_direct() {
    if !live_tests_enabled() {
        eprintln!("live_174_nav_verbs_resolve_from_events_direct: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = LiveFirefox::headless_on_random_port();
    exercise_events_path(&direct_args(ff.port()), "direct");
}

/// The daemon leg of the same assertions. It passed before this iteration too
/// — that is the point: it is the reference the direct route silently drifted
/// away from, and pinning both is what keeps them from drifting again.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_174_nav_verbs_resolve_from_events_daemon() {
    if !live_tests_enabled() {
        eprintln!("live_174_nav_verbs_resolve_from_events_daemon: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = LiveFirefox::headless_on_random_port();
    assert!(
        ff.with_daemon().is_some(),
        "live_174_nav_verbs_resolve_from_events_daemon: the proxy daemon did not \
         start for Firefox on port {}",
        ff.port()
    );
    let port = ff.port();
    exercise_events_path(&daemon_args(port), "daemon");
    stop_daemon(port);
}
