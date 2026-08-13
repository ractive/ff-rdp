//! Live tests for iter-131 — measurement honesty: perf transfer sizes,
//! responsive simulation fields, snapshot bounds, throttle state.
//!
//! ACs (see kb/iterations/iteration-131-measurement-honesty.md):
//!   - live_131_perf_opaque_transfer: cross-origin resources without
//!     Timing-Allow-Origin → per-resource `transfer_size: null`, aggregate
//!     `transfer_size_opaque: true`.
//!   - live_131_perf_transparent_transfer: same-origin resources → real sizes,
//!     `transfer_size_opaque: false`, aggregate equals the per-resource sum.
//!   - live_131_responsive_simulation_fields: `responsive` promotes
//!     `media_queries_applied` + `simulation` per breakpoint.
//!   - live_131_snapshot_max_chars_bounds: `snapshot --max-chars` bounds the
//!     WHOLE serialized tree, not just leaf text.
//!   - live_131_throttle_status: `throttle status` recalls the profile last
//!     applied via the daemon.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live live_131_measurement_honesty -- --nocapture

use std::process::Command;
use std::time::Duration;

use serde_json::Value;

use crate::common::{
    FixtureRoute, FixtureServer, LiveFirefox, base_args, ff_rdp_bin, live_tests_enabled,
};

/// A well-formed 1×1 transparent GIF (43 bytes) — small, real image bytes so
/// the browser fires `load` (not `error`) and Resource Timing records a
/// genuine `decodedBodySize`/`transferSize` for the same-origin case.
const PIXEL_GIF: &[u8] = &[
    0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 0x01, 0x00, 0x01, 0x00, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00,
    0xff, 0xff, 0xff, 0x21, 0xf9, 0x04, 0x01, 0x00, 0x00, 0x00, 0x00, 0x2c, 0x00, 0x00, 0x00, 0x00,
    0x01, 0x00, 0x01, 0x00, 0x00, 0x02, 0x02, 0x44, 0x01, 0x00, 0x3b,
];

/// Daemon-path args (no `--no-daemon`): commands share the persistent daemon
/// connection, so state set by one command (e.g. `throttle`'s bookkeeping) is
/// visible to the next. Mirrors `live_109_throttle_block.rs::daemon_args`.
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

fn stop_daemon(port: u16) {
    let _ = Command::new(ff_rdp_bin())
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "daemon",
            "stop",
        ])
        .output();
}

fn navigate(port: u16, url: &str) {
    let nav = Command::new(ff_rdp_bin())
        .args(base_args(port))
        .args(["navigate", url])
        .output()
        .expect("ff-rdp navigate");
    assert!(
        nav.status.success(),
        "navigate to {url} failed: {}",
        String::from_utf8_lossy(&nav.stderr)
    );
}

fn run_json(port: u16, args: &[&str]) -> Value {
    run_json_with(&base_args(port), args)
}

/// Like [`run_json`] but over the daemon path (see [`daemon_args`]) — used by
/// the throttle-status test, which needs state to persist across separate
/// `ff-rdp` invocations.
fn run_json_daemon(port: u16, args: &[&str]) -> Value {
    run_json_with(&daemon_args(port), args)
}

fn run_json_with(base: &[String], args: &[&str]) -> Value {
    let out = Command::new(ff_rdp_bin())
        .args(base)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("spawn ff-rdp {args:?}: {e}"));
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

/// Drop any resource whose URL contains "favicon" — headless Firefox
/// auto-requests `/favicon.ico` for any navigated page regardless of the
/// fixture's own markup, which would otherwise pollute resource-count
/// assertions with an extra, unrelated same-origin request.
fn without_favicon(results: &[Value]) -> Vec<Value> {
    results
        .iter()
        .filter(|r| !r["url"].as_str().is_some_and(|u| u.contains("favicon")))
        .cloned()
        .collect()
}

/// Poll `document.images` until `n` images exist and all have finished
/// loading (`.complete`), or `timeout` elapses. Resource Timing entries for
/// an `<img>` are recorded on network completion, not decode success, but
/// waiting for `complete` keeps the test from racing a still-in-flight fetch.
fn wait_for_images_loaded(port: u16, n: usize, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    let expr = format!(
        "document.images.length >= {n} && \
         Array.prototype.every.call(document.images, function(i) {{ return i.complete; }})"
    );
    loop {
        let out = Command::new(ff_rdp_bin())
            .args(base_args(port))
            .args(["eval", &expr])
            .output();
        if let Ok(o) = out
            && o.status.success()
        {
            let json: Result<Value, _> =
                serde_json::from_slice(&o.stdout).map(|v: Value| v["results"].clone());
            if json.ok() == Some(Value::Bool(true)) {
                return true;
            }
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

// ---------------------------------------------------------------------------
// Theme A — perf opaque vs. transparent transfer sizes
// ---------------------------------------------------------------------------

/// `live_131_perf_opaque_transfer`:
///
/// A page whose only resources are cross-origin images served without
/// `Timing-Allow-Origin` must yield per-resource `transfer_size: null` (not
/// `0`) and a top-level `transfer_size_opaque: true` — the Resource Timing
/// spec zeroes cross-origin sizes without that header; treating the zero as a
/// real measurement fabricates a "page weight" (dogfood-62 #3).
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_131_perf_opaque_transfer() {
    if !live_tests_enabled() {
        eprintln!("live_131_perf_opaque_transfer: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let Some(image_origin) = FixtureServer::start(std::collections::HashMap::from([(
        "/pixel.gif".to_owned(),
        FixtureRoute {
            content_type: "image/gif",
            body: PIXEL_GIF.to_vec(),
            extra_headers: Vec::new(), // no Timing-Allow-Origin — the point of the test
        },
    )])) else {
        eprintln!("live_131_perf_opaque_transfer: could not bind fixture server — skipping");
        return;
    };

    let page_html = format!(
        "<!DOCTYPE html><html><body>\
         <img src=\"{base}/pixel.gif?a=1\">\
         <img src=\"{base}/pixel.gif?a=2\">\
         </body></html>",
        base = image_origin.base_url()
    );
    let Some(page_origin) = FixtureServer::start(std::collections::HashMap::from([(
        "/".to_owned(),
        FixtureRoute::html(page_html),
    )])) else {
        eprintln!("live_131_perf_opaque_transfer: could not bind page server — skipping");
        return;
    };
    // Different port ⇒ different origin per same-origin policy, even though
    // both are 127.0.0.1 — this is what makes the images cross-origin.
    assert_ne!(image_origin.port(), page_origin.port());

    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();
    navigate(port, &page_origin.base_url());
    assert!(
        wait_for_images_loaded(port, 2, Duration::from_secs(10)),
        "live_131_perf_opaque_transfer: images did not finish loading in time"
    );

    // Per-resource: `perf` (default type=resource) must null every size for
    // the two cross-origin images. Headless Firefox auto-requests
    // `/favicon.ico` from the PAGE's own (same-)origin regardless of the
    // fixture's markup — filter it out so it doesn't pollute the count.
    let per_resource = run_json(port, &["perf"]);
    let all_results = per_resource["results"]
        .as_array()
        .expect("perf results array")
        .clone();
    let images = without_favicon(&all_results);
    assert_eq!(
        images.len(),
        2,
        "expected exactly 2 (non-favicon) resources: {per_resource}"
    );
    for r in &images {
        assert!(
            r["transfer_size"].is_null(),
            "opaque cross-origin resource must have transfer_size: null, not 0: {r}"
        );
        assert_eq!(
            r["transfer_size_opaque"], true,
            "opaque cross-origin resource must be flagged: {r}"
        );
    }

    // Aggregate: `perf summary` must flag the aggregate, not silently sum 0s.
    // Compare against values independently derived from the per-resource
    // list above (rather than hardcoding e.g. "count == 2") so the assertion
    // holds regardless of whether a favicon request happened to occur.
    let expected_opaque_count = all_results
        .iter()
        .filter(|r| r["transfer_size_opaque"] == true)
        .count() as u64;
    let expected_total: f64 = all_results
        .iter()
        .filter_map(|r| r["transfer_size"].as_f64())
        .sum();
    assert_eq!(
        expected_opaque_count, 2,
        "sanity: exactly the 2 cross-origin images should be opaque: {all_results:?}"
    );

    let summary = run_json(port, &["perf", "summary"]);
    assert_eq!(
        summary["results"]["transfer_size_opaque"], true,
        "aggregate must flag opaque transfer sizes: {summary}"
    );
    assert_eq!(
        summary["results"]["transfer_size_opaque_count"], expected_opaque_count,
        "opaque count must match the per-resource list: {summary}"
    );
    let total = summary["results"]["total_transfer_size"]
        .as_f64()
        .expect("total_transfer_size is a number");
    assert!(
        (total - expected_total).abs() < 0.01,
        "aggregate total ({total}) must equal the sum of NON-opaque per-resource sizes \
         ({expected_total}) — opaque resources must be excluded, not summed as 0: {summary}"
    );

    eprintln!("live_131_perf_opaque_transfer: PASSED — total={total} (opaque excluded)");
}

/// `live_131_perf_transparent_transfer`:
///
/// A same-origin fixture reports real (non-null, non-zero) per-resource
/// sizes, `transfer_size_opaque: false`, and an aggregate that equals the sum
/// of the per-resource sizes.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_131_perf_transparent_transfer() {
    if !live_tests_enabled() {
        eprintln!("live_131_perf_transparent_transfer: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let page_html = "<!DOCTYPE html><html><body>\
         <img src=\"/pixel.gif?a=1\">\
         </body></html>"
        .to_owned();
    let Some(server) = FixtureServer::start(std::collections::HashMap::from([
        ("/".to_owned(), FixtureRoute::html(page_html)),
        (
            "/pixel.gif".to_owned(),
            FixtureRoute {
                content_type: "image/gif",
                body: PIXEL_GIF.to_vec(),
                extra_headers: Vec::new(),
            },
        ),
    ])) else {
        eprintln!("live_131_perf_transparent_transfer: could not bind fixture server — skipping");
        return;
    };

    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();
    navigate(port, &server.base_url());
    assert!(
        wait_for_images_loaded(port, 1, Duration::from_secs(10)),
        "live_131_perf_transparent_transfer: image did not finish loading in time"
    );

    // Headless Firefox auto-requests `/favicon.ico` from the same origin
    // regardless of the fixture's markup, in addition to the one we asked
    // for — filter it out so the "exactly 1 resource" assertion isn't
    // environment-dependent.
    let per_resource = run_json(port, &["perf"]);
    let all_results = per_resource["results"]
        .as_array()
        .expect("perf results array")
        .clone();
    let images = without_favicon(&all_results);
    assert_eq!(
        images.len(),
        1,
        "expected exactly 1 (non-favicon) resource: {per_resource}"
    );
    let r = &images[0];
    assert!(
        r["transfer_size"].as_f64().is_some_and(|v| v > 0.0),
        "same-origin resource must report a real (>0) transfer_size: {r}"
    );
    assert_eq!(
        r["transfer_size_opaque"], false,
        "same-origin resource must not be flagged opaque: {r}"
    );
    // Every resource on this fixture is same-origin/real (including the
    // favicon Firefox added on its own) — the aggregate must equal the sum
    // of ALL of them, not just the one we asked for.
    let resource_sum: f64 = all_results
        .iter()
        .filter_map(|r| r["transfer_size"].as_f64())
        .sum();

    let summary = run_json(port, &["perf", "summary"]);
    assert_eq!(
        summary["results"]["transfer_size_opaque"], false,
        "aggregate must not be flagged opaque when nothing is: {summary}"
    );
    let total = summary["results"]["total_transfer_size"]
        .as_f64()
        .expect("total_transfer_size is a number");
    assert!(
        (total - resource_sum).abs() < 0.01,
        "aggregate total ({total}) must equal the sum of per-resource sizes ({resource_sum}): {summary}"
    );

    eprintln!("live_131_perf_transparent_transfer: PASSED — total={total} matches resource sum");
}

// ---------------------------------------------------------------------------
// Theme B — responsive simulation fields
// ---------------------------------------------------------------------------

/// `live_131_responsive_simulation_fields`:
///
/// `responsive body --widths 320` on a desktop-viewport Firefox reports
/// `media_queries_applied: false` and `simulation: "css-width-constraint"`
/// promoted to top-level breakpoint fields, next to `rect`/`elements` — not
/// buried only in the `warnings` array.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_131_responsive_simulation_fields() {
    if !live_tests_enabled() {
        eprintln!("live_131_responsive_simulation_fields: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = LiveFirefox::headless_on_random_port();
    navigate(ff.port(), "https://example.com");

    let json = run_json(ff.port(), &["responsive", "body", "--widths", "320"]);
    let bp = &json["results"]["breakpoints"][0];
    assert_eq!(bp["width"], 320);
    assert_eq!(
        bp["simulation"], "css-width-constraint",
        "breakpoint must name the simulation technique: {json}"
    );
    // Headless Firefox's default (wide) physical viewport does not flip media
    // queries to 320px under the layout-only CSS emulation (iter-98).
    assert_eq!(
        bp["media_queries_applied"], false,
        "media_queries_applied must be promoted and reflect the layout-only reality: {json}"
    );

    eprintln!("live_131_responsive_simulation_fields: PASSED");
}

// ---------------------------------------------------------------------------
// Theme C — snapshot --max-chars bounds the whole tree
// ---------------------------------------------------------------------------

/// `live_131_snapshot_max_chars_bounds`:
///
/// `snapshot --max-chars 500` on a page whose full snapshot exceeds 500
/// characters bounds the WHOLE serialized output (not just leaf text) to
/// roughly that budget and carries `truncated: true`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_131_snapshot_max_chars_bounds() {
    if !live_tests_enabled() {
        eprintln!("live_131_snapshot_max_chars_bounds: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    // Self-hosted (not a real-world origin, whose content can shrink over
    // time and flake this test): 60 sibling <div>s guarantee a serialized
    // tree comfortably over the 500-byte budget, deterministically.
    let mut rows = String::new();
    for i in 0..60 {
        use std::fmt::Write as _;
        let _ = write!(
            rows,
            "<div class=\"item-{i}\" data-testid=\"row-{i}\">row {i} content</div>"
        );
    }
    let big_page = format!("<!DOCTYPE html><html><body>{rows}</body></html>");
    let Some(server) = FixtureServer::start(std::collections::HashMap::from([(
        "/".to_owned(),
        FixtureRoute::html(big_page),
    )])) else {
        eprintln!("live_131_snapshot_max_chars_bounds: could not bind fixture server — skipping");
        return;
    };

    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();
    navigate(port, &server.base_url());

    // Baseline: the full (default --max-chars 50000) snapshot, to confirm
    // this page's tree genuinely exceeds a 500-byte budget.
    let full = run_json(port, &["snapshot"]);
    let full_len = serde_json::to_string(&full["results"])
        .unwrap_or_default()
        .len();
    assert!(
        full_len > 500,
        "fixture page's full snapshot ({full_len} bytes) must exceed the 500-byte budget \
         to exercise bounding — got too small a tree: {full}"
    );

    let bounded = run_json(port, &["snapshot", "--max-chars", "500"]);
    let bounded_len = serde_json::to_string(&bounded["results"])
        .unwrap_or_default()
        .len();

    assert!(
        bounded_len < full_len,
        "--max-chars 500 must shrink the WHOLE output below the full {full_len}-byte tree, \
         not just leaf text (s61 #9 near-no-op bug): bounded={bounded_len} full={full_len}"
    );
    // Generous envelope-overhead allowance per the AC's own wording.
    assert!(
        bounded_len <= 500 + 300,
        "bounded output ({bounded_len} bytes) should stay close to the 500-byte budget"
    );
    assert_eq!(
        bounded["results"]["truncated"], true,
        "truncated snapshot must carry truncated:true: {bounded}"
    );

    eprintln!(
        "live_131_snapshot_max_chars_bounds: PASSED — full={full_len} bytes, bounded={bounded_len} bytes"
    );
}

// ---------------------------------------------------------------------------
// Theme D — throttle status
// ---------------------------------------------------------------------------

/// `live_131_throttle_status`:
///
/// After `throttle slow-3g` (daemon path), `throttle status` reports
/// `profile: "slow-3g"`; after `throttle off` it reports `profile: null`
/// (none active) — `throttle` is no longer write-only.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_131_throttle_status() {
    if !live_tests_enabled() {
        eprintln!("live_131_throttle_status: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = LiveFirefox::headless_on_random_port();
    if ff.with_daemon().is_none() {
        eprintln!("live_131_throttle_status: daemon did not start — skipping");
        return;
    }
    let port = ff.port();

    // Every call here must go over the daemon path (no `--no-daemon`) —
    // throttle state is client-side bookkeeping keyed to *this* daemon
    // process (Theme D), so a one-shot connection would never see it.
    let applied = run_json_daemon(port, &["throttle", "slow-3g"]);
    assert_eq!(
        applied["results"]["profile"], "slow-3g",
        "throttle slow-3g must echo the applied profile: {applied}"
    );

    let status = run_json_daemon(port, &["throttle", "status"]);
    assert_eq!(
        status["results"]["profile"], "slow-3g",
        "throttle status must recall the last-applied profile: {status}"
    );
    assert!(
        status["results"]["cache_caveat"]
            .as_str()
            .is_some_and(|c| c.contains("cache")),
        "throttle status must carry the cache caveat: {status}"
    );

    let off = run_json_daemon(port, &["throttle", "off"]);
    assert_eq!(off["results"]["profile"], "off");

    let status_after_off = run_json_daemon(port, &["throttle", "status"]);
    assert!(
        status_after_off["results"]["profile"].is_null(),
        "after throttle off, status must report no active profile: {status_after_off}"
    );

    stop_daemon(port);
    eprintln!("live_131_throttle_status: PASSED");
}
