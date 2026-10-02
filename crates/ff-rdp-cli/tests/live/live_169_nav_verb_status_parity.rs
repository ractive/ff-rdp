//! Live tests for iteration 169 Theme B — `back`/`forward`/`reload` report no
//! status at all.
//!
//! iter-130 Theme B promised that all four navigation verbs report the same
//! envelope. They did not: `nav_action.rs` built
//! `{committed_url, ready_state, elapsed_ms}` and stopped, so
//! `--jq '.results.status'` on a `reload` yielded `null` — indistinguishable
//! from `navigate`'s *meaningful* `null`, and without the `status_reason`
//! iter-166 added specifically to tell those apart.
//!
//! Measured on `main` at 86262f0:
//!
//! ```text
//! ff-rdp reload --jq '.results | keys'
//! → ["action","committed_url","elapsed_ms","ready_state"]
//! ```
//!
//! These tests assert both keys are present on all three verbs, on the
//! commit-wait path and on `--no-wait`. They use a **local** fixture server so
//! the status assertion is against a response this test controls rather than
//! a real origin: a `reload` of a 200 page must report 200, not merely "some
//! number".
//!
//! iter-174: a direct connection used to be starved of `dom-complete` and could
//! only ever answer `not_observed` for `reload` — fixed by passing
//! `isServerTargetSwitchingEnabled` on `getWatcher`.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli --test live live_169 -- --nocapture

use std::collections::HashMap;
use std::process::{Command, Output};

use serde_json::Value;

use crate::common::{FixtureRoute, FixtureServer, LiveFirefox, ff_rdp_bin, live_tests_enabled};

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

/// Run `ff-rdp <global> <args…>` and return its `results` object, asserting
/// exit 0.
fn run_ok(global: &[String], args: &[&str], label: &str) -> Value {
    let out = Command::new(ff_rdp_bin())
        .args(global)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("{label}: spawn ff-rdp {args:?}: {e}"));
    assert!(
        out.status.success(),
        "{label}: `{args:?}` must exit 0; got: {}",
        combined(&out)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("{label}: output not JSON: {e}\n{stdout}"));
    v["results"].clone()
}

/// The iter-169 Theme B envelope invariant: both keys always present, exactly
/// one of them non-`null`.
fn assert_status_pair(results: &Value, label: &str) {
    assert!(
        results.get("status").is_some(),
        "{label}: `status` must always be present, got {results}"
    );
    assert!(
        results.get("status_reason").is_some(),
        "{label}: `status_reason` must always be present, got {results}"
    );
    let has_status = !results["status"].is_null();
    let has_reason = !results["status_reason"].is_null();
    assert!(
        has_status != has_reason,
        "{label}: exactly one of `status`/`status_reason` must be non-null, got {results}"
    );
}

fn fixture_routes() -> HashMap<String, FixtureRoute> {
    let mut routes = HashMap::new();
    routes.insert(
        "/a".to_owned(),
        FixtureRoute::html("<!doctype html><title>iter-169 A</title><h1>A</h1>"),
    );
    routes.insert(
        "/b".to_owned(),
        FixtureRoute::html("<!doctype html><title>iter-169 B</title><h1>B</h1>"),
    );
    routes
}

/// Exercise all three history verbs on one Firefox, over one connection mode.
///
/// Returns without asserting anything only when the fixture server cannot
/// bind — every other path asserts.
fn exercise_nav_verbs(global: &[String], route: &str) {
    let Some(server) = FixtureServer::start(fixture_routes()) else {
        panic!("{route}: could not bind the local fixture server");
    };
    let url_a = format!("{}/a", server.base_url());
    let url_b = format!("{}/b", server.base_url());

    run_ok(
        global,
        &["navigate", &url_a],
        &format!("{route}: navigate a"),
    );
    run_ok(
        global,
        &["navigate", &url_b],
        &format!("{route}: navigate b"),
    );

    // --- reload: a real request, so a real status ------------------------
    //
    // iter-174: before that iteration a direct connection received only
    // `will-navigate` — the three `dom-*` events never arrived, so the events
    // wait burnt its whole budget and the readystate fallback, which
    // correlates no document request, answered `not_observed` (measured:
    // 21 029 ms against a 30 000 ms `--timeout`). A `not_observed` here is that
    // regression coming back.
    let results = run_ok(global, &["reload"], &format!("{route}: reload"));
    assert_status_pair(&results, &format!("{route}: reload"));
    assert_eq!(
        results["status"],
        Value::from(200),
        "{route}: reload of a 200 page must report 200, got {results}"
    );

    // --- back / forward --------------------------------------------------
    // A history traversal may be served from BFCache, in which case there is
    // genuinely no document request and `no_document_request` is the honest
    // answer. So assert the *invariant* here, not a specific status — the
    // point of Theme B is that the caller can now see which case it got
    // instead of an unexplained `null`.
    let results = run_ok(global, &["back"], &format!("{route}: back"));
    assert_status_pair(&results, &format!("{route}: back"));

    let results = run_ok(global, &["forward"], &format!("{route}: forward"));
    assert_status_pair(&results, &format!("{route}: forward"));

    // --- --no-wait: cannot have observed anything, and says so -----------
    for verb in ["reload", "back", "forward"] {
        let label = format!("{route}: {verb} --no-wait");
        let results = run_ok(global, &[verb, "--no-wait"], &label);
        assert_status_pair(&results, &label);
        assert_eq!(
            results["status_reason"], "not_observed",
            "{label}: a verb that returns before any resource can arrive must \
             say `not_observed`, got {results}"
        );
    }
}

/// AC: "`back`/`forward`/`reload` emit `status` and `status_reason` on every
/// path, with a live test asserting both keys are present on all three verbs".
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_169_nav_verbs_report_status() {
    if !live_tests_enabled() {
        eprintln!("live_169_nav_verbs_report_status: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = LiveFirefox::headless_on_random_port();
    exercise_nav_verbs(&cli_args(ff.port()), "direct");
}
