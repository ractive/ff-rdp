//! Live tests for iter-133 — mobile viewports: `launch --window-size`,
//! `screenshot --window-size` batch capture.
//!
//! ACs (see kb/iterations/iteration-133-viewport-emulation.md):
//!   - live_133_launch_window_size_above_floor: `launch --headless
//!     --window-size 600x800` -> `eval innerWidth` == 600 and a
//!     `(max-width: 700px)` media query matches (true emulation >= 500px).
//!   - live_133_launch_window_size_floor_warning: `launch --headless
//!     --window-size 390x844` -> `eval innerWidth` in [390, 500] (platform
//!     floor clamp) AND the launch envelope reports the requested 390x844
//!     alongside a below-floor warning.
//!   - live_133_screenshot_batch_mobile: with the live tab on example.com,
//!     `screenshot --window-size 390x844` -> PNG exactly 390px wide;
//!     envelope `capture == "batch-window-size"`. Gated on
//!     FF_RDP_LIVE_NETWORK_TESTS=1 in addition to FF_RDP_LIVE_TESTS=1 (it
//!     navigates to a real external host), matching the convention every
//!     other network-touching live suite follows (see `live_109`, `live_130`).
//!
//! `--dppx` for the batch path was planned but dropped during
//! implementation: direct testing against Firefox 153.0.3 showed
//! `layout.css.devPixelsPerPx` has zero effect on the `--screenshot`
//! raster (confirmed with and without e10s) — see the addendum in
//! kb/research/viewport-emulation.md. `screenshot_has_no_dppx_flag`
//! (tests/e2e/screenshot.rs) covers the negative: the flag does not exist.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live live_133_viewport_emulation -- --nocapture
//!
//! `live_133_screenshot_batch_mobile` additionally requires
//! `FF_RDP_LIVE_NETWORK_TESTS=1` (it navigates to a real external host).

use std::process::Command;

use base64::Engine as _;
use serde_json::Value;

use crate::common::{
    LiveFirefox, base_args, ff_rdp_bin, live_network_tests_enabled, live_tests_enabled,
};

fn navigate(port: u16, url: &str) {
    let nav = Command::new(ff_rdp_bin())
        .args(base_args(port))
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
        .args(base_args(port))
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

/// Independent PNG-header width/height reader (does not reuse the
/// production `png_dimensions` helper in `commands::screenshot`) — the
/// point of the AC is to verify the ACTUAL raster dimensions against the
/// PNG spec's own IHDR chunk, not to re-check our own code's self-report.
fn png_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    if data.len() < 24 {
        return None;
    }
    let width = u32::from_be_bytes(data[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(data[20..24].try_into().ok()?);
    Some((width, height))
}

// ---------------------------------------------------------------------------
// Theme A — `launch --window-size`
// ---------------------------------------------------------------------------

/// `live_133_launch_window_size_above_floor`: `launch --headless
/// --window-size 600x800` gives a TRUE live viewport at a width >= the
/// ~500px floor — real `innerWidth`, real media-query evaluation.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_133_launch_window_size_above_floor() {
    if !live_tests_enabled() {
        eprintln!("live_133_launch_window_size_above_floor: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let (ff, launch_json) =
        LiveFirefox::headless_on_random_port_with_args(&["--window-size", "600x800"]);
    let port = ff.port();

    assert_eq!(
        launch_json["results"]["window_size"]["requested"]["width"], 600,
        "launch envelope must report the requested width: {launch_json}"
    );
    assert_eq!(
        launch_json["results"]["window_size"]["requested"]["height"], 800,
        "launch envelope must report the requested height: {launch_json}"
    );
    assert_eq!(
        launch_json["results"]["window_size"]["below_floor"], false,
        "600px is above the ~500px floor, so below_floor must be false: {launch_json}"
    );
    assert!(
        launch_json["results"]["warnings"].is_null(),
        "no below-floor warning expected above the floor: {launch_json}"
    );

    let inner = run_json(port, &["eval", "innerWidth"]);
    assert_eq!(
        inner["results"], 600,
        "innerWidth must be the TRUE requested width (600), not clamped: {inner}"
    );

    let mq = run_json(port, &["eval", "matchMedia('(max-width: 700px)').matches"]);
    assert_eq!(
        mq["results"], true,
        "a (max-width: 700px) media query must genuinely match at 600px: {mq}"
    );

    eprintln!("live_133_launch_window_size_above_floor: PASSED");
}

/// `live_133_launch_window_size_floor_warning`: `launch --headless
/// --window-size 390x844` (below the ~500px floor) clamps the LIVE viewport
/// up to the floor, and the launch envelope reports the requested size
/// alongside a below-floor warning — it never silently pretends the
/// request was honored.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_133_launch_window_size_floor_warning() {
    if !live_tests_enabled() {
        eprintln!("live_133_launch_window_size_floor_warning: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let (ff, launch_json) =
        LiveFirefox::headless_on_random_port_with_args(&["--window-size", "390x844"]);
    let port = ff.port();

    assert_eq!(
        launch_json["results"]["window_size"]["requested"]["width"], 390,
        "launch envelope must report the requested (not clamped) width: {launch_json}"
    );
    assert_eq!(
        launch_json["results"]["window_size"]["requested"]["height"], 844,
        "launch envelope must report the requested (not clamped) height: {launch_json}"
    );
    assert_eq!(
        launch_json["results"]["window_size"]["below_floor"], true,
        "390px is below the ~500px floor, so below_floor must be true: {launch_json}"
    );
    let warnings = launch_json["results"]["warnings"]
        .as_array()
        .unwrap_or_else(|| {
            panic!("launch envelope must carry a below-floor warning: {launch_json}")
        });
    assert!(
        !warnings.is_empty(),
        "warnings array must be non-empty: {launch_json}"
    );

    let inner = run_json(port, &["eval", "innerWidth"]);
    let inner_width = inner["results"]
        .as_u64()
        .unwrap_or_else(|| panic!("innerWidth must be numeric: {inner}"));
    assert!(
        (390..=500).contains(&inner_width),
        "innerWidth ({inner_width}) must be clamped into [390, 500] on the live \
         debugger-server instance: {inner}"
    );

    eprintln!("live_133_launch_window_size_floor_warning: PASSED — innerWidth={inner_width}");
}

// ---------------------------------------------------------------------------
// Theme B — `screenshot --window-size` batch capture
// ---------------------------------------------------------------------------

/// `live_133_screenshot_batch_mobile`: with the live tab on example.com,
/// `screenshot --window-size 390x844` produces a PNG EXACTLY 390px wide
/// (verified against the PNG's own IHDR chunk, not the command's
/// self-report), and the envelope names the capture mode.
///
/// Navigates to a real external host (`example.com`) both over the live RDP
/// tab and via the batch-capture subprocess, so — matching every other
/// network-touching live suite (`live_109`, `live_111`, `live_130`) — this
/// is additionally gated on `FF_RDP_LIVE_NETWORK_TESTS=1`. Without it,
/// `FF_RDP_LIVE_TESTS=1 cargo test-live` alone must not reach the network.
#[test]
#[ignore = "requires a live Firefox instance and network — set FF_RDP_LIVE_TESTS=1 and FF_RDP_LIVE_NETWORK_TESTS=1"]
fn live_133_screenshot_batch_mobile() {
    if !live_tests_enabled() || !live_network_tests_enabled() {
        eprintln!(
            "live_133_screenshot_batch_mobile: set FF_RDP_LIVE_TESTS=1 and FF_RDP_LIVE_NETWORK_TESTS=1"
        );
        return;
    }
    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();
    navigate(port, "https://example.com");

    let json = run_json(
        port,
        &["screenshot", "--window-size", "390x844", "--base64"],
    );
    assert_eq!(
        json["results"]["capture"], "batch-window-size",
        "envelope must self-identify the batch capture mode: {json}"
    );

    let b64 = json["results"]["base64"]
        .as_str()
        .unwrap_or_else(|| panic!("base64 field missing: {json}"));
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .expect("valid base64 PNG");
    let (width, height) = png_dimensions(&bytes).expect("valid PNG IHDR");
    assert_eq!(
        width, 390,
        "batch-captured PNG must be EXACTLY 390px wide (no floor): got {width}x{height}"
    );

    eprintln!("live_133_screenshot_batch_mobile: PASSED — {width}x{height}");
}
