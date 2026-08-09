//! iter-130 Theme A/B/C — navigation truthfulness live tests.
//!
//! Theme A: `committed_url` reflects the real committed document on SPA route
//! commits instead of a literal `about:blank` placeholder (comparis.ch).
//! Theme B: `back`/`forward`/`reload` return the same navigate-style envelope
//! (`committed_url`, `ready_state`, `elapsed_ms`) as `navigate`.
//! Theme C: `perf summary` never reports a bare unmarked `total_resources: 0`
//! right after `reload` — either the buffer has caught up, or
//! `resources_pending: true` says so explicitly.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli --test live live_130 -- --nocapture
//!   FF_RDP_LIVE_NETWORK_TESTS=1 cargo test -p ff-rdp-cli --test live live_130_spa_committed_url -- --include-ignored --nocapture

use std::collections::HashMap;
use std::process::Command;

use crate::common::{
    FixtureRoute, FixtureServer, LiveFirefox, base_args, ff_rdp_bin, live_network_tests_enabled,
    live_tests_enabled,
};

fn parse_results(out: &std::process::Output) -> serde_json::Value {
    let s = String::from_utf8_lossy(&out.stdout);
    let top: serde_json::Value = serde_json::from_str(s.trim()).unwrap_or_else(|e| {
        panic!(
            "stdout is not valid JSON: {e}\nstdout={s}\nstderr={}",
            String::from_utf8_lossy(&out.stderr)
        )
    });
    top["results"].clone()
}

/// AC: `live_130_spa_committed_url` (network-gated) — navigate
/// `https://www.comparis.ch/hypotheken` → `committed_url` starts with
/// `https://www.comparis.ch` and is not `about:blank`.
///
/// Pre-fix repro (dogfooding-session-61 #5): the comparis SPA route-commit
/// flow reported a literal `"about:blank"` `committed_url` while
/// `ready_state: complete` and a manual `eval location.href` both confirmed
/// the real page had landed — a caller trusting `committed_url` would
/// wrongly conclude the navigation failed.
#[test]
#[ignore = "requires Firefox + network — FF_RDP_LIVE_NETWORK_TESTS=1"]
fn live_130_spa_committed_url() {
    if !live_network_tests_enabled() {
        eprintln!("live_130_spa_committed_url: set FF_RDP_LIVE_NETWORK_TESTS=1 to run");
        return;
    }
    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_130_spa_committed_url: Firefox not available — skipping");
        return;
    };

    let nav = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["navigate", "https://www.comparis.ch/hypotheken"])
        .output()
        .expect("navigate comparis.ch/hypotheken");
    if !nav.status.success() {
        eprintln!(
            "live_130_spa_committed_url: navigate failed (site may be unreachable or changed) \
             — {} — skipping rather than failing on a live-site issue",
            String::from_utf8_lossy(&nav.stderr)
        );
        return;
    }

    let results = parse_results(&nav);
    let committed = results["committed_url"].as_str().unwrap_or("");

    // Diagnostic aid: if committed_url is wrong, dump the live document's own
    // view from a fresh connection to distinguish "the real docshell
    // genuinely reports about:blank" from "our wait loop is looking at a
    // stale/wrong actor" — this exact distinction is what caught the
    // cross-process transient-about:blank-docshell race during iter-130
    // development (unreachable from any mock-based unit test).
    if !committed.starts_with("https://www.comparis.ch") {
        for (label, script) in [
            ("location.href", "window.location.href"),
            ("readyState", "document.readyState"),
            ("title", "document.title"),
        ] {
            let out = Command::new(ff_rdp_bin())
                .args(base_args(ff.port()))
                .args(["eval", script])
                .output()
                .expect("debug eval");
            eprintln!(
                "DEBUG {label}: stdout={} stderr={}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
        }
    }

    assert!(
        committed.starts_with("https://www.comparis.ch"),
        "committed_url must start with https://www.comparis.ch, got {committed:?}: {results}"
    );
    assert_ne!(
        committed, "about:blank",
        "committed_url must not be the literal about:blank placeholder: {results}"
    );
}

/// AC: `live_130_back_forward_envelope` — on local fixture pages, `back` and
/// `forward` each return `committed_url` matching the landing page, a
/// `ready_state`, and `elapsed_ms` > 0.
#[test]
#[ignore = "requires Firefox — FF_RDP_LIVE_TESTS=1"]
fn live_130_back_forward_envelope() {
    if !live_tests_enabled() {
        eprintln!("live_130_back_forward_envelope: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }
    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_130_back_forward_envelope: Firefox not available — skipping");
        return;
    };

    let mut routes = HashMap::new();
    routes.insert(
        "/a".to_owned(),
        FixtureRoute::html("<!doctype html><title>iter-130 A</title><h1>A</h1>"),
    );
    routes.insert(
        "/b".to_owned(),
        FixtureRoute::html("<!doctype html><title>iter-130 B</title><h1>B</h1>"),
    );
    let Some(server) = FixtureServer::start(routes) else {
        eprintln!("live_130_back_forward_envelope: could not bind local HTTP — skipping");
        return;
    };
    let url_a = format!("{}/a", server.base_url());
    let url_b = format!("{}/b", server.base_url());
    let ff_args = || base_args(ff.port());

    let nav_a = Command::new(ff_rdp_bin())
        .args(ff_args())
        .args(["navigate", &url_a])
        .output()
        .expect("navigate A");
    assert!(
        nav_a.status.success(),
        "live_130_back_forward_envelope: navigate A failed — {}",
        String::from_utf8_lossy(&nav_a.stderr)
    );

    let nav_b = Command::new(ff_rdp_bin())
        .args(ff_args())
        .args(["navigate", &url_b])
        .output()
        .expect("navigate B");
    assert!(
        nav_b.status.success(),
        "live_130_back_forward_envelope: navigate B failed — {}",
        String::from_utf8_lossy(&nav_b.stderr)
    );

    // back → lands on A.
    let back = Command::new(ff_rdp_bin())
        .args(ff_args())
        .args(["back"])
        .output()
        .expect("back");
    assert!(
        back.status.success(),
        "live_130_back_forward_envelope: back failed — {}",
        String::from_utf8_lossy(&back.stderr)
    );
    let back_results = parse_results(&back);
    assert_eq!(
        back_results["action"], "back",
        "back envelope: {back_results}"
    );
    let back_url = back_results["committed_url"].as_str().unwrap_or("");
    assert!(
        back_url.ends_with("/a"),
        "back should land on /a, got {back_url:?}: {back_results}"
    );
    assert!(
        back_results["ready_state"].is_string(),
        "back must carry a ready_state: {back_results}"
    );
    assert!(
        back_results["elapsed_ms"].as_u64().unwrap_or(0) > 0,
        "back elapsed_ms must be > 0: {back_results}"
    );

    // forward → lands on B.
    let fwd = Command::new(ff_rdp_bin())
        .args(ff_args())
        .args(["forward"])
        .output()
        .expect("forward");
    assert!(
        fwd.status.success(),
        "live_130_back_forward_envelope: forward failed — {}",
        String::from_utf8_lossy(&fwd.stderr)
    );
    let fwd_results = parse_results(&fwd);
    assert_eq!(
        fwd_results["action"], "forward",
        "forward envelope: {fwd_results}"
    );
    let fwd_url = fwd_results["committed_url"].as_str().unwrap_or("");
    assert!(
        fwd_url.ends_with("/b"),
        "forward should land on /b, got {fwd_url:?}: {fwd_results}"
    );
    assert!(
        fwd_results["ready_state"].is_string(),
        "forward must carry a ready_state: {fwd_results}"
    );
    assert!(
        fwd_results["elapsed_ms"].as_u64().unwrap_or(0) > 0,
        "forward elapsed_ms must be > 0: {fwd_results}"
    );
}

/// AC: `live_130_reload_envelope` — `reload` returns the navigate-style
/// envelope with `ready_state: "complete"` on a static fixture.
#[test]
#[ignore = "requires Firefox — FF_RDP_LIVE_TESTS=1"]
fn live_130_reload_envelope() {
    if !live_tests_enabled() {
        eprintln!("live_130_reload_envelope: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }
    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_130_reload_envelope: Firefox not available — skipping");
        return;
    };

    let mut routes = HashMap::new();
    routes.insert(
        "/page".to_owned(),
        FixtureRoute::html("<!doctype html><title>iter-130 reload</title><h1>hi</h1>"),
    );
    let Some(server) = FixtureServer::start(routes) else {
        eprintln!("live_130_reload_envelope: could not bind local HTTP — skipping");
        return;
    };
    let url = format!("{}/page", server.base_url());
    let ff_args = || base_args(ff.port());

    let nav = Command::new(ff_rdp_bin())
        .args(ff_args())
        .args(["navigate", &url])
        .output()
        .expect("navigate");
    assert!(
        nav.status.success(),
        "live_130_reload_envelope: navigate failed — {}",
        String::from_utf8_lossy(&nav.stderr)
    );

    let reload = Command::new(ff_rdp_bin())
        .args(ff_args())
        .args(["reload"])
        .output()
        .expect("reload");
    assert!(
        reload.status.success(),
        "live_130_reload_envelope: reload failed — {}",
        String::from_utf8_lossy(&reload.stderr)
    );

    let results = parse_results(&reload);
    assert_eq!(results["action"], "reload", "reload envelope: {results}");
    assert_eq!(
        results["ready_state"], "complete",
        "reload must report ready_state: complete: {results}"
    );
    let committed = results["committed_url"].as_str().unwrap_or("");
    assert!(
        committed.ends_with("/page"),
        "reload's committed_url should be the reloaded page, got {committed:?}: {results}"
    );
    assert!(
        results["elapsed_ms"].is_u64(),
        "reload elapsed_ms must be present: {results}"
    );
}

/// AC: `live_130_perf_no_silent_zero` — `reload` followed immediately by
/// `perf summary` either reports `total_resources` > 0 or carries the
/// explicit pending/no-data marker — never a bare unmarked 0.
#[test]
#[ignore = "requires Firefox — FF_RDP_LIVE_TESTS=1"]
fn live_130_perf_no_silent_zero() {
    if !live_tests_enabled() {
        eprintln!("live_130_perf_no_silent_zero: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }
    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_130_perf_no_silent_zero: Firefox not available — skipping");
        return;
    };

    let mut routes = HashMap::new();
    routes.insert(
        "/page".to_owned(),
        FixtureRoute::html(
            "<!doctype html><title>iter-130 perf</title>\
             <link rel=\"stylesheet\" href=\"/style.css\"><h1>hi</h1>",
        ),
    );
    routes.insert(
        "/style.css".to_owned(),
        FixtureRoute {
            content_type: "text/css",
            body: b"body{color:red}".to_vec(),
            extra_headers: Vec::new(),
        },
    );
    let Some(server) = FixtureServer::start(routes) else {
        eprintln!("live_130_perf_no_silent_zero: could not bind local HTTP — skipping");
        return;
    };
    let url = format!("{}/page", server.base_url());
    let ff_args = || base_args(ff.port());

    let nav = Command::new(ff_rdp_bin())
        .args(ff_args())
        .args(["navigate", &url])
        .output()
        .expect("navigate");
    assert!(
        nav.status.success(),
        "live_130_perf_no_silent_zero: navigate failed — {}",
        String::from_utf8_lossy(&nav.stderr)
    );

    let reload = Command::new(ff_rdp_bin())
        .args(ff_args())
        .args(["reload"])
        .output()
        .expect("reload");
    assert!(
        reload.status.success(),
        "live_130_perf_no_silent_zero: reload failed — {}",
        String::from_utf8_lossy(&reload.stderr)
    );

    // Immediately query perf summary — the exact race window Theme C guards.
    let summary = Command::new(ff_rdp_bin())
        .args(ff_args())
        .args(["perf", "summary"])
        .output()
        .expect("perf summary");
    assert!(
        summary.status.success(),
        "live_130_perf_no_silent_zero: perf summary failed — {}",
        String::from_utf8_lossy(&summary.stderr)
    );

    let results = parse_results(&summary);
    assert!(
        results.get("resources_pending").is_some(),
        "resources_pending must always be present, even when total_resources > 0: {results}"
    );
    let total = results["total_resources"].as_u64().unwrap_or(0);
    let pending = results["resources_pending"].as_bool().unwrap_or(false);
    assert!(
        total > 0 || pending,
        "perf summary right after reload must report total_resources > 0 OR \
         resources_pending: true — never a bare unmarked 0: {results}"
    );
}
