//! iter-125: `perf audit` must agree with `perf vitals` on LCP.
//!
//! Regression from [[dogfooding-session-61]]: on a page where LCP is
//! unmeasurable, `perf vitals` correctly reported `lcp_ms: null,
//! lcp_rating: "unavailable"`, but `perf audit` on the **same page** reported
//! `lcp_ms: 0.0, lcp_rating: "good"` — a false all-clear. The two commands had
//! duplicated LCP logic that drifted; iter-125 routes both through the shared
//! `apply_lcp_fields` helper so they cannot disagree.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live live_perf_audit_lcp_unavailable -- --nocapture
use std::collections::HashMap;
use std::process::Command;

use crate::common::{
    FixtureRoute, FixtureServer, LiveFirefox, base_args, ff_rdp_bin, live_tests_enabled,
};

/// Text-only fixture: no `<img>/<video>/<svg>/<canvas>` and no
/// `background-image`, so the LCP DOM-approximation fallback (layer 3) finds no
/// candidate element and the `largest-contentful-paint` entry list stays empty.
/// `compute_lcp` therefore returns `None` → the N7 guard fires → LCP is
/// `"unavailable"` / `null` in both commands. This is the exact
/// comparis.ch-class case where the drift produced a false `"good"` / `0.0`.
fn text_only_body() -> String {
    "<!DOCTYPE html><html><head><title>iter-125 lcp unavailable</title></head>\
     <body>\
     <h1>No largest-contentful-paint candidate here.</h1>\
     <p>Text only — no img, video, svg, canvas, or background-image, so the DOM \
     LCP approximation finds nothing and LCP is genuinely unmeasurable.</p>\
     </body></html>"
        .to_owned()
}

fn spawn_html_server(body: String) -> Option<FixtureServer> {
    let mut routes = HashMap::new();
    routes.insert("/".to_owned(), FixtureRoute::html(body));
    FixtureServer::start(routes)
}

/// Extract the audit vitals LCP fields under `results.vitals`.
fn audit_lcp(v: &serde_json::Value) -> (&serde_json::Value, &str) {
    let vitals = v
        .pointer("/results/vitals")
        .or_else(|| v.pointer("/results/0/vitals"))
        .unwrap_or_else(|| panic!("perf audit output missing results.vitals: {v}"));
    let lcp_ms = &vitals["lcp_ms"];
    let lcp_rating = vitals["lcp_rating"].as_str().unwrap_or("missing");
    (lcp_ms, lcp_rating)
}

/// Extract the vitals LCP fields under `results` (single-object envelope).
fn vitals_lcp(v: &serde_json::Value) -> (&serde_json::Value, &str) {
    let results = v
        .pointer("/results")
        .or_else(|| v.pointer("/results/0"))
        .unwrap_or_else(|| panic!("perf vitals output missing results: {v}"));
    let lcp_ms = &results["lcp_ms"];
    let lcp_rating = results["lcp_rating"].as_str().unwrap_or("missing");
    (lcp_ms, lcp_rating)
}

/// `live_perf_audit_lcp_unavailable`:
/// On a text-only page with no LCP candidate, `perf audit` must report
/// `.results.vitals.lcp_rating == "unavailable"` and `.results.vitals.lcp_ms ==
/// null` — never `"good"` / `0.0` (the pre-iter-125 false all-clear).
///
/// Gated on `FF_RDP_LIVE_TESTS=1`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_perf_audit_lcp_unavailable() {
    if !live_tests_enabled() {
        eprintln!("live_perf_audit_lcp_unavailable: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_perf_audit_lcp_unavailable: Firefox not available — skipping");
        return;
    };

    let Some(server) = spawn_html_server(text_only_body()) else {
        eprintln!("live_perf_audit_lcp_unavailable: could not bind HTTP server — skipping");
        return;
    };
    let url = server.base_url();

    let nav = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["navigate", &url])
        .output()
        .expect("ff-rdp navigate failed");
    assert!(
        nav.status.success(),
        "live_perf_audit_lcp_unavailable: navigate failed — {}",
        String::from_utf8_lossy(&nav.stderr)
    );

    let perf_out = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["perf", "audit"])
        .output()
        .expect("ff-rdp perf audit failed");
    assert!(
        perf_out.status.success(),
        "live_perf_audit_lcp_unavailable: perf audit failed — {}",
        String::from_utf8_lossy(&perf_out.stderr)
    );

    let perf_json: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&perf_out.stdout))
            .expect("perf audit is not valid JSON");
    let (lcp_ms, lcp_rating) = audit_lcp(&perf_json);

    // The false all-clear: rating "good" with a zero-or-null lcp_ms.
    assert!(
        !(lcp_rating == "good" && (lcp_ms.is_null() || lcp_ms.as_f64() == Some(0.0))),
        "live_perf_audit_lcp_unavailable: FAIL — audit reports a false 'good' \
         all-clear (lcp_ms={lcp_ms}) on a page with no LCP candidate"
    );

    if lcp_ms.is_null() {
        assert_eq!(
            lcp_rating, "unavailable",
            "live_perf_audit_lcp_unavailable: lcp_ms is null but lcp_rating is \
             {lcp_rating:?} — expected 'unavailable'"
        );
        eprintln!("live_perf_audit_lcp_unavailable: PASS — lcp_rating=unavailable, lcp_ms=null");
    } else {
        // If this Firefox build somehow surfaced a real LCP, it must be a
        // sensible positive value, not the fabricated zero.
        let ms = lcp_ms.as_f64().unwrap_or(-1.0);
        assert!(
            ms > 0.0,
            "live_perf_audit_lcp_unavailable: lcp_ms={lcp_ms} must be positive when present"
        );
        eprintln!(
            "live_perf_audit_lcp_unavailable: PASS (LCP surfaced) — lcp_rating={lcp_rating:?}, lcp_ms={ms}ms"
        );
    }
}

/// `live_perf_audit_vitals_lcp_parity`:
/// On the same page in the same session, `perf audit`'s
/// `.results.vitals.{lcp_ms, lcp_rating}` must equal `perf vitals`'
/// `.results.{lcp_ms, lcp_rating}` field-for-field. This is the direct guard
/// against the two commands drifting again.
///
/// Gated on `FF_RDP_LIVE_TESTS=1`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_perf_audit_vitals_lcp_parity() {
    if !live_tests_enabled() {
        eprintln!("live_perf_audit_vitals_lcp_parity: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_perf_audit_vitals_lcp_parity: Firefox not available — skipping");
        return;
    };

    let Some(server) = spawn_html_server(text_only_body()) else {
        eprintln!("live_perf_audit_vitals_lcp_parity: could not bind HTTP server — skipping");
        return;
    };
    let url = server.base_url();

    let nav = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["navigate", &url])
        .output()
        .expect("ff-rdp navigate failed");
    assert!(
        nav.status.success(),
        "live_perf_audit_vitals_lcp_parity: navigate failed — {}",
        String::from_utf8_lossy(&nav.stderr)
    );

    let vitals_out = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["perf", "vitals"])
        .output()
        .expect("ff-rdp perf vitals failed");
    assert!(
        vitals_out.status.success(),
        "live_perf_audit_vitals_lcp_parity: perf vitals failed — {}",
        String::from_utf8_lossy(&vitals_out.stderr)
    );

    let audit_out = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["perf", "audit"])
        .output()
        .expect("ff-rdp perf audit failed");
    assert!(
        audit_out.status.success(),
        "live_perf_audit_vitals_lcp_parity: perf audit failed — {}",
        String::from_utf8_lossy(&audit_out.stderr)
    );

    let vitals_json: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&vitals_out.stdout))
            .expect("perf vitals is not valid JSON");
    let audit_json: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&audit_out.stdout))
            .expect("perf audit is not valid JSON");

    let (v_ms, v_rating) = vitals_lcp(&vitals_json);
    let (a_ms, a_rating) = audit_lcp(&audit_json);

    assert_eq!(
        v_rating, a_rating,
        "live_perf_audit_vitals_lcp_parity: lcp_rating drift — vitals={v_rating:?} \
         audit={a_rating:?}"
    );
    assert_eq!(
        v_ms, a_ms,
        "live_perf_audit_vitals_lcp_parity: lcp_ms drift — vitals={v_ms} audit={a_ms}"
    );
    eprintln!(
        "live_perf_audit_vitals_lcp_parity: PASS — lcp_rating={a_rating:?}, lcp_ms={a_ms} match"
    );
}
