//! Live tests for iteration 295 — `perf --cold` and `perf compare` errors
//! that name the URL and the step (feedback 2026-10-05).
//!
//! The fixture pages load a stylesheet and a script served with
//! `Cache-Control: max-age=3600`, so a second plain load is served from the
//! HTTP cache (`transferSize` 0) and only a cache-bypassing load reports bytes.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live live_295_perf \
//!       -- --include-ignored --nocapture

use std::collections::HashMap;
use std::process::Command;
use std::time::Duration;

use serde_json::Value;

use crate::common::{FixtureRoute, FixtureServer, LiveFirefox, ff_rdp_bin, live_tests_enabled};

const CACHEABLE: &str = "max-age=3600";

fn cacheable(content_type: &'static str, body: &str) -> FixtureRoute {
    FixtureRoute {
        content_type,
        body: body.as_bytes().to_vec(),
        ..FixtureRoute::default()
    }
    .with_header("Cache-Control", CACHEABLE)
}

fn page(title: &str) -> FixtureRoute {
    FixtureRoute::html(format!(
        "<!DOCTYPE html><html><head><link rel=\"stylesheet\" href=\"/style.css\">\
         <script src=\"/app.js\"></script></head><body><h1>{title}</h1></body></html>"
    ))
    .with_header("Cache-Control", CACHEABLE)
}

/// Two pages sharing a cacheable stylesheet and script.
fn start_fixture() -> Option<FixtureServer> {
    let padding = "x".repeat(2048);
    FixtureServer::start(HashMap::from([
        ("/a.html".to_owned(), page("page a")),
        ("/b.html".to_owned(), page("page b")),
        (
            "/style.css".to_owned(),
            cacheable(
                "text/css",
                &format!("h1 {{ color: #333; }} /* {padding} */"),
            ),
        ),
        (
            "/app.js".to_owned(),
            cacheable(
                "text/javascript",
                &format!("window.appLoaded = true; // {padding}"),
            ),
        ),
        (
            "/slow.html".to_owned(),
            FixtureRoute::html("<!DOCTYPE html><p>slow</p>").with_delay(Duration::from_secs(4)),
        ),
    ]))
}

fn run(port: u16, timeout_ms: u64, args: &[&str]) -> std::process::Output {
    Command::new(ff_rdp_bin())
        .args(["--host", "127.0.0.1", "--port", &port.to_string()])
        .args(["--timeout", &timeout_ms.to_string()])
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("spawn ff-rdp {args:?}: {e}"))
}

fn run_json(port: u16, args: &[&str]) -> Value {
    let out = run(port, 30_000, args);
    assert!(
        out.status.success(),
        "command {args:?} failed: {}",
        crate::common::output_note(&out)
    );
    serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("output for {args:?} not JSON: {e}"))
}

fn summary_bytes(json: &Value) -> f64 {
    json["results"]["total_transfer_size"]
        .as_f64()
        .unwrap_or_else(|| panic!("total_transfer_size missing: {json}"))
}

/// After two plain loads the page is cached (`total_transfer_size` 0);
/// `perf summary --cold` reloads it with the cache bypassed and reports bytes
/// plus `meta.cache: "bypassed"`; `perf vitals --cold` / `perf audit --cold`
/// carry the same marker.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_295_perf_summary_cold_bypasses_cache() {
    if !live_tests_enabled() {
        eprintln!("live_295_perf_summary_cold_bypasses_cache: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();
    let Some(server) = start_fixture() else {
        eprintln!("live_295_perf_summary_cold_bypasses_cache: could not bind fixture server");
        return;
    };
    let url = format!("{}/a.html", server.base_url());
    run_json(port, &["navigate", &url]);
    run_json(port, &["navigate", &url]);

    let warm = run_json(port, &["perf", "summary"]);
    assert_eq!(
        summary_bytes(&warm),
        0.0,
        "second load must be cached: {warm}"
    );
    assert!(warm["meta"].get("cache").is_none(), "{warm}");

    let cold = run_json(port, &["perf", "summary", "--cold"]);
    assert!(
        summary_bytes(&cold) > 0.0,
        "--cold must transfer bytes: {cold}"
    );
    assert_eq!(cold["meta"]["cache"], "bypassed", "{cold}");

    for sub in ["vitals", "audit"] {
        let json = run_json(port, &["perf", sub, "--cold"]);
        assert_eq!(json["meta"]["cache"], "bypassed", "perf {sub}: {json}");
    }
    let audit = run_json(port, &["perf", "audit", "--cold"]);
    assert!(
        audit["results"]["navigation"]["transfer_size"]
            .as_f64()
            .is_some_and(|n| n > 0.0),
        "perf audit --cold must re-fetch the document: {audit}"
    );
}

/// `perf compare --cold` reports bytes for both URLs; the override dies with
/// the connection, so a later plain load of the same page is cached again.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_295_perf_compare_cold_then_cached_again() {
    if !live_tests_enabled() {
        eprintln!("live_295_perf_compare_cold_then_cached_again: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();
    let Some(server) = start_fixture() else {
        eprintln!("live_295_perf_compare_cold_then_cached_again: could not bind fixture server");
        return;
    };
    let a = format!("{}/a.html", server.base_url());
    let b = format!("{}/b.html", server.base_url());
    // Warm the cache for both pages.
    run_json(port, &["perf", "compare", &a, &b]);

    let cold = run_json(port, &["perf", "compare", "--cold", &a, &b]);
    assert_eq!(cold["meta"]["cache"], "bypassed", "{cold}");
    let results = cold["results"].as_array().expect("results array");
    assert_eq!(results.len(), 2, "{cold}");
    for r in results {
        assert!(
            r["resources"]["total_transfer_size"]
                .as_f64()
                .is_some_and(|n| n > 0.0),
            "--cold must transfer bytes for {}: {cold}",
            r["url"]
        );
    }

    run_json(port, &["navigate", &b]);
    let after = run_json(port, &["perf", "summary"]);
    assert_eq!(
        summary_bytes(&after),
        0.0,
        "the cache bypass must end with the --cold command's connection: {after}"
    );
}

/// A `perf compare` step that outlives `--timeout` fails naming the URL and
/// the step, never a bare `RdpTimeout { phase: "recv" }`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_295_perf_compare_timeout_names_url_and_step() {
    if !live_tests_enabled() {
        eprintln!("live_295_perf_compare_timeout_names_url_and_step: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();
    let Some(server) = start_fixture() else {
        eprintln!("live_295_perf_compare_timeout_names_url_and_step: could not bind server");
        return;
    };
    let a = format!("{}/a.html", server.base_url());
    let slow = format!("{}/slow.html", server.base_url());
    let out = run(port, 1500, &["perf", "compare", &a, &slow]);
    assert!(
        !out.status.success(),
        "a 4 s page must not fit a 1.5 s step"
    );
    let json: Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "error envelope not JSON: {e}: {}",
            crate::common::output_note(&out)
        )
    });
    let msg = json["error"].as_str().unwrap_or_default();
    assert_eq!(json["error_type"], "Timeout", "{json}");
    // Normally the slow page; under a loaded parallel run the fast page's
    // 1.5 s step can run out first, and the error must then name that one.
    assert!(
        msg.contains(&slow) || msg.contains(&a),
        "error must name the URL: {msg}"
    );
    assert!(
        msg.contains("step `navigate`") || msg.contains("step `readystate`"),
        "error must name the step: {msg}"
    );
    assert!(msg.contains("not the whole run"), "{msg}");
}
