//! iter-82 AC: `perf_vitals_emits_unavailable_when_lcp_missing`.
//!
//! On headless Firefox, PerformanceObserver does not surface LCP entries.
//! This test asserts that `ff-rdp perf vitals` emits `lcp_rating == "unavailable"`
//! and `lcp_ms == null` in that scenario.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live live_perf_vitals_headless -- --nocapture

use std::process::Command;

use crate::common::{LiveFirefox, base_args, ff_rdp_bin};

/// `perf_vitals_emits_unavailable_when_lcp_missing`:
/// On headless Firefox where PerformanceObserver doesn't report LCP entries,
/// `ff-rdp perf vitals` must emit `lcp_rating == "unavailable"` and
/// `lcp_ms == null` (N7 fix) instead of `"good"` / `0.0`.
///
/// Gated on `FF_RDP_LIVE_TESTS=1`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_perf_vitals_lcp_unavailable_when_lcp_missing() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("perf_vitals_emits_unavailable_when_lcp_missing: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!(
            "perf_vitals_emits_unavailable_when_lcp_missing: Firefox not available — skipping"
        );
        return;
    };

    // Navigate to a simple page so perf vitals has something to measure.
    let nav = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["navigate", "about:blank"])
        .output()
        .expect("ff-rdp navigate");
    assert!(
        nav.status.success(),
        "perf_vitals_emits_unavailable_when_lcp_missing: navigate failed — {}",
        String::from_utf8_lossy(&nav.stderr)
    );

    let out = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["perf", "vitals"])
        .output()
        .expect("ff-rdp perf vitals");
    assert!(
        out.status.success(),
        "perf_vitals_emits_unavailable_when_lcp_missing: perf vitals failed — stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    let json: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
        panic!(
            "perf_vitals_emits_unavailable_when_lcp_missing: output is not valid JSON: \
                 {e}\nstdout={stdout}\nstderr={}",
            String::from_utf8_lossy(&out.stderr)
        )
    });

    let vitals = &json["results"];

    // On headless Firefox LCP is typically unavailable.  We accept either:
    //   1. lcp_rating == "unavailable" and lcp_ms == null (the expected N7 path)
    //   2. lcp_rating is a valid string AND lcp_ms is a non-negative number
    //      (if headless Firefox happens to surface LCP on this build)
    //
    // What we must NOT see is lcp_rating == "good" with lcp_ms == 0 or null,
    // which was the pre-N7 bug.
    let lcp_rating = vitals["lcp_rating"].as_str().unwrap_or("missing");
    let lcp_ms = &vitals["lcp_ms"];

    if lcp_ms.is_null() {
        assert_eq!(
            lcp_rating, "unavailable",
            "perf_vitals_emits_unavailable_when_lcp_missing: lcp_ms is null but \
             lcp_rating is {lcp_rating:?} — expected 'unavailable'"
        );
        eprintln!(
            "perf_vitals_emits_unavailable_when_lcp_missing: PASS — lcp_rating=unavailable, lcp_ms=null"
        );
    } else {
        // LCP was surfaced — assert it's a sensible positive number.
        let ms = lcp_ms.as_f64().unwrap_or(-1.0);
        assert!(
            ms > 0.0,
            "perf_vitals_emits_unavailable_when_lcp_missing: lcp_ms={lcp_ms} must be \
             positive when present"
        );
        // And lcp_rating must not be "good" with a near-zero value (the N7 regression).
        assert!(
            !(ms < 1.0 && lcp_rating == "good"),
            "perf_vitals_emits_unavailable_when_lcp_missing: lcp_ms={ms}ms < 1ms \
             rates as 'good' — this looks like the pre-N7 zero-value bug"
        );
        eprintln!(
            "perf_vitals_emits_unavailable_when_lcp_missing: PASS (LCP surfaced) — \
             lcp_rating={lcp_rating:?}, lcp_ms={ms}ms"
        );
    }
}
