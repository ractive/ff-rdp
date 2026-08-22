//! Live tests for iteration 139 — perf honesty II: unmeasurable vitals, byte
//! attribution, page identity.
//!
//! Follow-up to [[iteration-131-measurement-honesty]] and
//! [[iteration-125-perf-audit-lcp-unavailable]] — same false-good class,
//! reproduced live against real headless Firefox during implementation
//! (bbc.com/news, en.wikipedia.org) before any code changed, per
//! [[dogfooding-session-63]]:
//!
//! - Theme A: `cls`/`tbt_ms` fabricated "good" `0.0` — Firefox's
//!   `PerformanceObserver.supportedEntryTypes` has neither `layout-shift` nor
//!   `longtask`, structurally, not merely on this page.
//! - Theme B: `resource_by_type.document` (a few hundred bytes, populated only
//!   by incidental ad iframes) contradicted `navigation.transfer_size` (tens
//!   of KB); `third_party_summary.count` read as 100% because the navigation
//!   document itself — always first-party — was invisible to that breakdown.
//! - Theme C: `perf vitals` carried no URL/timestamp, so stale
//!   (pre-navigation) numbers were indistinguishable from fresh ones.
//! - Theme D: `perf summary --format text` "Top 5 Slowest Resources" printed
//!   raw (untruncated) URLs — lines of 6000+ chars on ad-heavy pages.
//!
//! daemon-parity: every test here uses [`daemon_args`] (no `--no-daemon`) —
//! these commands go through `evaluateJSAsync` on the already-resolved tab
//! target, the same path iter-137 fixed for the default connection mode.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live live_139_perf_honesty_2 -- --nocapture

use std::collections::HashMap;
use std::process::Command;
use std::time::Duration;

use serde_json::Value;

use crate::common::{FixtureRoute, FixtureServer, LiveFirefox, ff_rdp_bin, live_tests_enabled};

/// A well-formed 1×1 transparent GIF (43 bytes) — same fixture iter-131 uses.
const PIXEL_GIF: &[u8] = &[
    0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 0x01, 0x00, 0x01, 0x00, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00,
    0xff, 0xff, 0xff, 0x21, 0xf9, 0x04, 0x01, 0x00, 0x00, 0x00, 0x00, 0x2c, 0x00, 0x00, 0x00, 0x00,
    0x01, 0x00, 0x01, 0x00, 0x00, 0x02, 0x02, 0x44, 0x01, 0x00, 0x3b,
];

/// Args for the **default** connection mode: no `--no-daemon`, so the CLI
/// auto-starts and proxies through the daemon — the path every real
/// invocation uses (see the module-level `daemon-parity` note).
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
        .args(["--host", "127.0.0.1", "--port", &port.to_string()])
        .args(["daemon", "stop"])
        .output();
}

/// Bring up Firefox with a running daemon.
///
/// Panics on either failure (iter-158 Theme D) — the `Option` this used to
/// return made every caller `return` early, which libtest reports as `ok`.
fn firefox_with_daemon(test: &str) -> LiveFirefox {
    let ff = LiveFirefox::headless_on_random_port();
    assert!(
        ff.with_daemon().is_some(),
        "{test}: the proxy daemon did not start for Firefox on port {}",
        ff.port()
    );
    ff
}

fn navigate(port: u16, url: &str) {
    let nav = Command::new(ff_rdp_bin())
        .args(daemon_args(port))
        .args(["navigate", url])
        .output()
        .expect("ff-rdp navigate");
    assert!(
        nav.status.success(),
        "navigate to {url} failed: {}",
        crate::common::output_note(&nav)
    );
}

fn run_json(port: u16, args: &[&str]) -> Value {
    let out = Command::new(ff_rdp_bin())
        .args(daemon_args(port))
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

fn run_text(port: u16, args: &[&str]) -> String {
    let out = Command::new(ff_rdp_bin())
        .args(daemon_args(port))
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("spawn ff-rdp {args:?}: {e}"));
    assert!(
        out.status.success(),
        "command {args:?} failed: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Poll `document.images` until `n` images exist and have all finished
/// loading, or `timeout` elapses. Mirrors `live_131_measurement_honesty`'s
/// helper of the same name.
fn wait_for_images_loaded(port: u16, n: usize, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    let expr = format!(
        "document.images.length >= {n} && \
         Array.prototype.every.call(document.images, function(i) {{ return i.complete; }})"
    );
    loop {
        let out = Command::new(ff_rdp_bin())
            .args(daemon_args(port))
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
// Theme A — CLS/TBT unavailable, never a fabricated "good"
// ---------------------------------------------------------------------------

/// `live_139_cls_unavailable`: on a real (default-daemon-path) page, `perf
/// vitals` must report `cls: null` / `cls_rating: "unavailable"` with a note
/// naming `layout-shift` — never the old `0.0` / `"good"` false all-clear.
/// `perf audit`'s `vitals.cls` must agree.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_139_cls_unavailable() {
    if !live_tests_enabled() {
        eprintln!("live_139_cls_unavailable: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_139_cls_unavailable");
    let port = ff.port();
    navigate(port, "https://example.com");

    let vitals = run_json(port, &["perf", "vitals"]);
    assert!(
        vitals["results"]["cls"].is_null(),
        "cls must be null, never a measured value Firefox cannot produce: {vitals}"
    );
    assert_eq!(
        vitals["results"]["cls_rating"], "unavailable",
        "cls_rating must be 'unavailable', never 'good': {vitals}"
    );
    assert!(
        vitals["results"]["cls_note"]
            .as_str()
            .is_some_and(|n| n.contains("layout-shift")),
        "cls_note must name the missing entry type: {vitals}"
    );

    let audit = run_json(port, &["perf", "audit"]);
    assert_eq!(
        audit["results"]["vitals"]["cls_rating"], "unavailable",
        "perf audit must agree with perf vitals on cls: {audit}"
    );

    stop_daemon(port);
    eprintln!("live_139_cls_unavailable: PASSED");
}

/// `live_139_tbt_unavailable`: twin of the above for TBT / `longtask`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_139_tbt_unavailable() {
    if !live_tests_enabled() {
        eprintln!("live_139_tbt_unavailable: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_139_tbt_unavailable");
    let port = ff.port();
    navigate(port, "https://example.com");

    let vitals = run_json(port, &["perf", "vitals"]);
    assert!(
        vitals["results"]["tbt_ms"].is_null(),
        "tbt_ms must be null, never a measured value Firefox cannot produce: {vitals}"
    );
    assert_eq!(
        vitals["results"]["tbt_rating"], "unavailable",
        "tbt_rating must be 'unavailable', never 'good': {vitals}"
    );
    assert!(
        vitals["results"]["tbt_note"]
            .as_str()
            .is_some_and(|n| n.contains("longtask")),
        "tbt_note must name the missing entry type: {vitals}"
    );

    let audit = run_json(port, &["perf", "audit"]);
    assert_eq!(
        audit["results"]["vitals"]["tbt_rating"], "unavailable",
        "perf audit must agree with perf vitals on tbt: {audit}"
    );

    stop_daemon(port);
    eprintln!("live_139_tbt_unavailable: PASSED");
}

// ---------------------------------------------------------------------------
// Theme B — byte attribution
// ---------------------------------------------------------------------------

/// `live_139_audit_document_bytes_agree`: `resource_by_type.document` must
/// not contradict `navigation.transfer_size` — before iter-139, the top-level
/// HTML document (a `navigation` entry, never a `resource` entry) was
/// entirely excluded from `resource_by_type`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_139_audit_document_bytes_agree() {
    if !live_tests_enabled() {
        eprintln!("live_139_audit_document_bytes_agree: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_139_audit_document_bytes_agree");
    let port = ff.port();

    let Some(server) = FixtureServer::start(HashMap::from([(
        "/".to_owned(),
        FixtureRoute::html("<!DOCTYPE html><html><body><h1>plain</h1></body></html>".to_owned()),
    )])) else {
        eprintln!("live_139_audit_document_bytes_agree: could not bind fixture server — skipping");
        return;
    };
    navigate(port, &server.base_url());

    let audit = run_json(port, &["perf", "audit"]);
    let nav_transfer_size = audit["results"]["navigation"]["transfer_size"]
        .as_f64()
        .expect("navigation.transfer_size must be a number for a same-origin document");
    let by_type = audit["results"]["resource_by_type"]
        .as_array()
        .expect("resource_by_type array");
    let doc_entry = by_type
        .iter()
        .find(|t| t["type"] == "document")
        .unwrap_or_else(|| {
            panic!(
                "resource_by_type must include a 'document' bucket for the \
                 navigation entry itself: {audit}"
            )
        });
    let doc_transfer_size = doc_entry["transfer_size"]
        .as_f64()
        .expect("document transfer_size must be a number");
    assert!(
        doc_transfer_size >= nav_transfer_size - 0.01,
        "resource_by_type.document ({doc_transfer_size}) must not be smaller than \
         navigation.transfer_size ({nav_transfer_size}) — the old bug undercounted \
         the page's own bytes: {audit}"
    );

    stop_daemon(port);
    eprintln!(
        "live_139_audit_document_bytes_agree: PASSED — document={doc_transfer_size} \
         navigation={nav_transfer_size}"
    );
}

/// `live_139_audit_opaque_flagged_per_type`: a cross-origin image without
/// `Timing-Allow-Origin` must flag `transfer_size_opaque: true` on the
/// `resource_by_type` bucket it falls into (Theme B point 2) — before
/// iter-139 only the top-level `resource_summary` carried this marker, so a
/// per-type total dominated by opaque placeholders read as real bytes.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_139_audit_opaque_flagged_per_type() {
    if !live_tests_enabled() {
        eprintln!("live_139_audit_opaque_flagged_per_type: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_139_audit_opaque_flagged_per_type");
    let port = ff.port();

    let Some(image_origin) = FixtureServer::start(HashMap::from([(
        "/pixel.gif".to_owned(),
        FixtureRoute {
            content_type: "image/gif",
            body: PIXEL_GIF.to_vec(),
            extra_headers: Vec::new(), // no Timing-Allow-Origin — the point
        },
    )])) else {
        eprintln!("live_139_audit_opaque_flagged_per_type: could not bind image server — skipping");
        return;
    };
    let page_html = format!(
        "<!DOCTYPE html><html><body><img src=\"{base}/pixel.gif\"></body></html>",
        base = image_origin.base_url()
    );
    let Some(page_origin) = FixtureServer::start(HashMap::from([(
        "/".to_owned(),
        FixtureRoute::html(page_html),
    )])) else {
        eprintln!("live_139_audit_opaque_flagged_per_type: could not bind page server — skipping");
        return;
    };
    assert_ne!(image_origin.port(), page_origin.port());

    navigate(port, &page_origin.base_url());
    assert!(
        wait_for_images_loaded(port, 1, Duration::from_secs(10)),
        "live_139_audit_opaque_flagged_per_type: image did not finish loading in time"
    );

    let audit = run_json(port, &["perf", "audit"]);
    let by_type = audit["results"]["resource_by_type"]
        .as_array()
        .expect("resource_by_type array");
    let image_entry = by_type
        .iter()
        .find(|t| t["type"] == "image")
        .unwrap_or_else(|| panic!("resource_by_type must include an 'image' bucket: {audit}"));
    assert_eq!(
        image_entry["transfer_size_opaque"], true,
        "the 'image' bucket must flag transfer_size_opaque when it contains an \
         opaque cross-origin resource: {audit}"
    );
    assert!(
        image_entry["transfer_size_opaque_count"]
            .as_u64()
            .is_some_and(|n| n >= 1),
        "transfer_size_opaque_count must count the opaque resource: {audit}"
    );

    let by_domain = audit["results"]["resource_by_domain"]
        .as_array()
        .expect("resource_by_domain array");
    let image_domain_entry = by_domain
        .iter()
        .find(|d| d["transfer_size_opaque"] == true)
        .unwrap_or_else(|| {
            panic!("resource_by_domain must flag the opaque image's domain: {audit}")
        });
    eprintln!(
        "live_139_audit_opaque_flagged_per_type: PASSED — image bucket and domain {:?} both flagged",
        image_domain_entry["domain"]
    );

    stop_daemon(port);
}

/// `live_139_third_party_excludes_first_party`: on a page with genuine
/// same-origin resources, `third_party_summary.count` must be strictly less
/// than the total resource count. Before iter-139 the navigation document
/// itself — always first-party — was invisible to this breakdown, so a page
/// with no OTHER same-hostname subresources read as "100% third-party".
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_139_third_party_excludes_first_party() {
    if !live_tests_enabled() {
        eprintln!("live_139_third_party_excludes_first_party: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_139_third_party_excludes_first_party");
    let port = ff.port();

    // Single-origin fixture: the page and its only sub-resource share a
    // hostname+port, so — together with the now-included navigation entry —
    // there are at least 2 first-party resources against 0 third-party ones.
    let page_html = "<!DOCTYPE html><html><body><img src=\"/pixel.gif\"></body></html>".to_owned();
    let Some(server) = FixtureServer::start(HashMap::from([
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
        eprintln!(
            "live_139_third_party_excludes_first_party: could not bind fixture server — skipping"
        );
        return;
    };
    navigate(port, &server.base_url());
    assert!(
        wait_for_images_loaded(port, 1, Duration::from_secs(10)),
        "live_139_third_party_excludes_first_party: image did not finish loading in time"
    );

    let audit = run_json(port, &["perf", "audit"]);
    let total = audit["results"]["resource_summary"]["count"]
        .as_u64()
        .expect("resource_summary.count must be a number");
    let third_party = audit["results"]["third_party_summary"]["count"]
        .as_u64()
        .expect("third_party_summary.count must be a number");
    assert!(
        third_party < total,
        "third_party_summary.count ({third_party}) must be less than the total \
         ({total}) on a page with genuine same-origin resources — the navigation \
         document itself is always first-party: {audit}"
    );

    stop_daemon(port);
    eprintln!(
        "live_139_third_party_excludes_first_party: PASSED — third_party={third_party} \
         total={total}"
    );
}

// ---------------------------------------------------------------------------
// Theme C — page identity
// ---------------------------------------------------------------------------

/// `live_139_vitals_page_identity`: `perf vitals` output must name the URL it
/// measured, so stale (pre-navigation) numbers are detectable instead of
/// silently looking current.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_139_vitals_page_identity() {
    if !live_tests_enabled() {
        eprintln!("live_139_vitals_page_identity: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_139_vitals_page_identity");
    let port = ff.port();

    let Some(server) = FixtureServer::start(HashMap::from([(
        "/".to_owned(),
        FixtureRoute::html("<!DOCTYPE html><html><body>id</body></html>".to_owned()),
    )])) else {
        eprintln!("live_139_vitals_page_identity: could not bind fixture server — skipping");
        return;
    };
    let url = server.base_url();
    navigate(port, &url);

    let before = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock")
        .as_millis();
    let vitals = run_json(port, &["perf", "vitals"]);
    let after = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock")
        .as_millis();

    let page_url = vitals["results"]["page_url"]
        .as_str()
        .unwrap_or_else(|| panic!("perf vitals must report page_url: {vitals}"));
    assert!(
        page_url.starts_with(&url),
        "page_url ({page_url}) must name the navigated URL ({url}): {vitals}"
    );

    let measured_at_ms = vitals["results"]["measured_at_ms"]
        .as_u64()
        .unwrap_or_else(|| panic!("perf vitals must report measured_at_ms: {vitals}"));
    assert!(
        u128::from(measured_at_ms) >= before && u128::from(measured_at_ms) <= after,
        "measured_at_ms ({measured_at_ms}) must fall within the call's wall-clock \
         window [{before}, {after}]: {vitals}"
    );

    stop_daemon(port);
    eprintln!("live_139_vitals_page_identity: PASSED — page_url={page_url}");
}

// ---------------------------------------------------------------------------
// Theme D — perf summary --format text line length
// ---------------------------------------------------------------------------

/// `live_139_perf_summary_text_bounded`: on a page whose slowest resource has
/// a pathologically long (ad-tracker-style) URL, `perf summary --format
/// text`'s longest line must stay bounded — iter-128's `middle_ellipsis` was
/// wired into `network`/`sources` but not here (session-62 #2 recurrence,
/// dogfood-63 #13: lines of 6709/7378 chars on real ad-heavy pages).
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_139_perf_summary_text_bounded() {
    if !live_tests_enabled() {
        eprintln!("live_139_perf_summary_text_bounded: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_139_perf_summary_text_bounded");
    let port = ff.port();

    // The server matches routes on path only (query string stripped), so the
    // full request URL the browser actually fetches — and Resource Timing
    // records — carries a 7000+-char query string, reproducing a real
    // ad-tracker-style URL without depending on a live third-party origin.
    let long_query = "x".repeat(7000);
    let page_html = format!(
        "<!DOCTYPE html><html><body><img src=\"/pixel.gif?id={long_query}\"></body></html>"
    );
    let Some(server) = FixtureServer::start(HashMap::from([
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
        eprintln!("live_139_perf_summary_text_bounded: could not bind fixture server — skipping");
        return;
    };
    navigate(port, &server.base_url());
    assert!(
        wait_for_images_loaded(port, 1, Duration::from_secs(10)),
        "live_139_perf_summary_text_bounded: image did not finish loading in time"
    );

    let text = run_text(port, &["perf", "summary", "--format", "text"]);
    let longest = text.lines().map(str::len).max().unwrap_or(0);
    assert!(
        longest < 200,
        "longest 'perf summary --format text' line ({longest} chars) must be bounded \
         (~120), not thousands — full output:\n{text}"
    );

    stop_daemon(port);
    eprintln!("live_139_perf_summary_text_bounded: PASSED — longest line = {longest} chars");
}
