//! Live tests for iteration 295 — `navigate --wait-idle [--idle-ms N]`.
//!
//! Before iter-295 `navigate` returned at `dom-complete`, so a page that
//! fetched data or inserted images after `load` was still loading when the
//! command returned, and users fell back to `wait --sleep-ms`. These tests
//! drive a local fixture page whose post-load `fetch` and image are both
//! held back by the server, and a page that never goes quiet.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live live_295 \
//!       -- --include-ignored --nocapture

use std::collections::HashMap;
use std::process::{Command, Output};
use std::time::Duration;

use serde_json::Value;

use crate::common::{FixtureRoute, FixtureServer, LiveFirefox, ff_rdp_bin, live_tests_enabled};

/// A 1×1 transparent PNG.
const PNG_1X1: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

/// `/delayed`: 300 ms after `load`, fetch `/slow-data` (held 300 ms by the
/// server); once it resolves, insert `<img src=/slow.png>` (held 300 ms).
/// Every step starts inside the default 500 ms quiet window of the one before
/// it, so a correct idle wait sees both requests.
///
/// `/busy`: fetches `/tick` every 50 ms forever — the network never goes quiet.
fn routes() -> HashMap<String, FixtureRoute> {
    let mut routes = HashMap::new();
    routes.insert(
        "/delayed".to_owned(),
        FixtureRoute::html(
            "<html><body><h1>delayed</h1><script>\
             addEventListener('load', () => setTimeout(() => {\
               fetch('/slow-data').then(r => r.text()).then(() => {\
                 window.__fetchDone = true;\
                 const img = new Image();\
                 img.src = '/slow.png';\
                 document.body.appendChild(img);\
               });\
             }, 300));\
             </script></body></html>",
        ),
    );
    routes.insert(
        "/slow-data".to_owned(),
        FixtureRoute {
            content_type: "text/plain",
            body: b"data".to_vec(),
            ..FixtureRoute::default()
        }
        .with_delay(Duration::from_millis(300)),
    );
    routes.insert(
        "/slow.png".to_owned(),
        FixtureRoute {
            content_type: "image/png",
            body: PNG_1X1.to_vec(),
            ..FixtureRoute::default()
        }
        .with_delay(Duration::from_millis(300)),
    );
    routes.insert(
        "/busy".to_owned(),
        FixtureRoute::html(
            "<html><body><h1>busy</h1><script>\
             setInterval(() => fetch('/tick?' + Date.now()), 50);\
             </script></body></html>",
        ),
    );
    routes.insert(
        "/lazy".to_owned(),
        FixtureRoute::html(
            "<html><body><h1>lazy</h1><div style=\"height: 6000px\"></div>\
             <img loading=\"lazy\" src=\"/slow.png\" width=\"10\" height=\"10\">\
             </body></html>",
        ),
    );
    routes.insert(
        "/tick".to_owned(),
        FixtureRoute {
            content_type: "text/plain",
            body: b"tick".to_vec(),
            ..FixtureRoute::default()
        },
    );
    routes
}

fn run(port: u16, args: &[&str]) -> Output {
    Command::new(ff_rdp_bin())
        .args(["--host", "127.0.0.1", "--port", &port.to_string()])
        .args(["--timeout", "30000"])
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("spawn ff-rdp {args:?}: {e}"))
}

fn combined(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn json_of(out: &Output) -> Value {
    serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("output not JSON: {e}\n{}", combined(out)))
}

/// AC: `navigate <fixture> --wait-idle` returns only after the delayed fetch
/// and the delayed image have both landed.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_295_navigate_wait_idle_waits_for_fetch_and_image() {
    if !live_tests_enabled() {
        eprintln!("live_295_navigate_wait_idle_waits_for_fetch_and_image: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let server = FixtureServer::start(routes()).expect("bind fixture server");
    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();
    let url = format!("{}/delayed", server.base_url());

    let out = run(port, &["navigate", &url, "--wait-idle"]);
    assert!(
        out.status.success(),
        "navigate --wait-idle: {}",
        combined(&out)
    );
    let idle = json_of(&out)["results"]["idle"].clone();
    assert_eq!(idle["images_complete"], true, "idle: {idle}");
    assert!(
        idle["requests_observed"].as_u64().unwrap_or(0) >= 2,
        "the fetch and the image must both be observed: {idle}"
    );
    assert!(idle["idle_at_ms"].as_u64().is_some(), "idle: {idle}");

    // Read the page state on a fresh connection: both requests must have
    // completed before `navigate` returned, with no extra waiting here.
    let out = run(
        port,
        &[
            "eval",
            "JSON.stringify({fetched: !!window.__fetchDone, images: document.images.length, \
             loaded: [...document.images].every(i => i.complete && i.naturalWidth > 0)})",
            "--unwrap",
        ],
    );
    assert!(out.status.success(), "eval: {}", combined(&out));
    let state = json_of(&out)["results"].clone();
    assert_eq!(state["fetched"], true, "fetch had not landed: {state}");
    assert_eq!(state["images"], 1, "image had not been inserted: {state}");
    assert_eq!(state["loaded"], true, "image had not loaded: {state}");
}

/// AC: a page that never goes quiet exhausts `--timeout-ms` and fails with a
/// timeout naming `--wait-idle`, never a success envelope.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_295_navigate_wait_idle_busy_page_times_out() {
    if !live_tests_enabled() {
        eprintln!("live_295_navigate_wait_idle_busy_page_times_out: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let server = FixtureServer::start(routes()).expect("bind fixture server");
    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();
    let url = format!("{}/busy", server.base_url());

    let out = run(
        port,
        &[
            "navigate",
            &url,
            "--wait-idle",
            "--idle-ms",
            "100",
            "--timeout-ms",
            "200",
        ],
    );
    assert!(
        !out.status.success(),
        "a page that never goes quiet must fail: {}",
        combined(&out)
    );
    let err = json_of(&out);
    assert_eq!(err["error_type"], "Timeout", "{err}");
    let msg = err["error"].as_str().unwrap_or_default();
    assert!(
        msg.contains("--wait-idle"),
        "error must name --wait-idle: {msg}"
    );
    assert!(err.get("results").is_none(), "no success envelope: {err}");
}

/// A `loading="lazy"` image far below the fold is never requested, and Firefox
/// reports it `complete === false`. `--wait-idle` must not wait on it (it
/// would always exhaust the budget) and must say it skipped it.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_295_navigate_wait_idle_skips_offscreen_lazy_images() {
    if !live_tests_enabled() {
        eprintln!(
            "live_295_navigate_wait_idle_skips_offscreen_lazy_images: set FF_RDP_LIVE_TESTS=1"
        );
        return;
    }
    let server = FixtureServer::start(routes()).expect("bind fixture server");
    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();
    let url = format!("{}/lazy", server.base_url());

    let out = run(port, &["navigate", &url, "--wait-idle"]);
    assert!(
        out.status.success(),
        "navigate --wait-idle: {}",
        combined(&out)
    );
    let idle = json_of(&out)["results"]["idle"].clone();
    assert_eq!(idle["images_complete"], true, "idle: {idle}");
    assert_eq!(idle["lazy_images_deferred"], 1, "idle: {idle}");
}
