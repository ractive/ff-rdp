//! iter-92 Theme A — live tests for `screenshot --full-page` correctness.
//!
//! Pre-fix repro: on the iter-89 process-drawsnapshot path, `--full-page` was
//! hard-rejected with an error; on the standard `screenshotActor.capture` path
//! the `fullPage:true` flag was forwarded but the resulting PNG was still
//! viewport-sized (byte-identical to a non-`--full-page` capture).
//!
//! These tests exercise the fix: after iter-92 both paths must produce a PNG
//! taller than the viewport when the page is 4 000 px tall.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live_92_screenshot_full_page -- --nocapture

#[path = "common/mod.rs"]
mod common;

use std::process::Command;

use base64::Engine as _;
use common::{LiveFirefox, base_args, ff_rdp_bin};

const TALL_PAGE_URL: &str = "data:text/html,<html><body style=\"height:4000px;background:linear-gradient(to bottom,red,blue)\">x</body></html>";

fn parse_results_json(out: &std::process::Output) -> serde_json::Value {
    let s = String::from_utf8_lossy(&out.stdout);
    let top: serde_json::Value = serde_json::from_str(s.trim()).unwrap_or_else(|e| {
        panic!(
            "stdout is not valid JSON: {e}\nstdout={s}\nstderr={}",
            String::from_utf8_lossy(&out.stderr)
        )
    });
    top["results"].clone()
}

fn png_height_from_bytes(data: &[u8]) -> Option<u32> {
    if data.len() < 24 {
        return None;
    }
    Some(u32::from_be_bytes(data[20..24].try_into().ok()?))
}

/// `pre_fix_repro_screenshot_full_page_taller_than_viewport`:
///
/// Navigate to a 4 000 px tall data-URL page and capture with `--full-page`.
/// Assert PNG height ≥ scrollHeight × DPR - 1 (allow one-row rounding).
///
/// Pre-fix behaviour: either returned an error ("not yet supported on the
/// Firefox 151 process-drawsnapshot fallback") or silently produced a
/// viewport-sized PNG byte-identical to a non-`--full-page` capture.
/// Post-fix: PNG height must exceed the viewport (~683 px at 1×DPR).
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn pre_fix_repro_screenshot_full_page_taller_than_viewport() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!(
            "pre_fix_repro_screenshot_full_page_taller_than_viewport: \
             set FF_RDP_LIVE_TESTS=1 to run"
        );
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!(
            "pre_fix_repro_screenshot_full_page_taller_than_viewport: \
             Firefox not available — skipping"
        );
        return;
    };

    let ff_args = || base_args(ff.port());

    // Navigate to the tall page.
    let nav = Command::new(ff_rdp_bin())
        .args(ff_args())
        .args(["navigate", TALL_PAGE_URL])
        .output()
        .expect("navigate to tall page");
    assert!(
        nav.status.success(),
        "pre_fix_repro_screenshot_full_page_taller_than_viewport: \
         navigate failed — {}",
        String::from_utf8_lossy(&nav.stderr)
    );

    // Query scrollHeight and DPR via eval.
    let scroll_eval = Command::new(ff_rdp_bin())
        .args(ff_args())
        .args([
            "eval",
            "JSON.stringify({scrollH: document.documentElement.scrollHeight, dpr: window.devicePixelRatio || 1})",
        ])
        .output()
        .expect("eval scrollHeight+dpr");
    assert!(
        scroll_eval.status.success(),
        "pre_fix_repro_screenshot_full_page_taller_than_viewport: \
         scrollHeight eval failed — {}",
        String::from_utf8_lossy(&scroll_eval.stderr)
    );

    let eval_json = {
        let s = String::from_utf8_lossy(&scroll_eval.stdout);
        let top: serde_json::Value =
            serde_json::from_str(s.trim()).expect("eval output should be JSON");
        // result is a JSON string containing the stringified object
        let inner_str = top["results"]["result"].as_str().unwrap_or_else(|| {
            // Grip::Value wraps the string result
            top["results"].as_str().unwrap_or("{}")
        });
        serde_json::from_str::<serde_json::Value>(inner_str).unwrap_or_else(|_| {
            // fallback: parse top-level results if already object
            top["results"].clone()
        })
    };

    let scroll_h = eval_json
        .get("scrollH")
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(4000.0);
    let dpr = eval_json
        .get("dpr")
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(1.0);

    // Take full-page screenshot in base64 mode.
    let out = Command::new(ff_rdp_bin())
        .args(ff_args())
        .args(["screenshot", "--full-page", "--base64"])
        .output()
        .expect("screenshot --full-page --base64");
    assert!(
        out.status.success(),
        "pre_fix_repro_screenshot_full_page_taller_than_viewport: \
         screenshot --full-page exited non-zero — {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let results = parse_results_json(&out);
    let png_height = results["height"]
        .as_u64()
        .expect("results.height must be a number");

    // Allow 1-row rounding.
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let min_height = ((scroll_h * dpr) as u64).saturating_sub(1);
    assert!(
        png_height >= min_height,
        "pre_fix_repro_screenshot_full_page_taller_than_viewport: \
         PNG height {png_height} < scrollHeight({scroll_h}) × DPR({dpr}) - 1 = {min_height}"
    );

    // Cross-check via PNG IHDR.
    let b64 = results["base64"]
        .as_str()
        .expect("results.base64 must be present with --base64");
    let png_bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .expect("valid base64");
    let ihdr_height =
        png_height_from_bytes(&png_bytes).expect("PNG IHDR height must be extractable");
    let ihdr_min = u32::try_from(min_height).unwrap_or(u32::MAX);
    assert!(
        ihdr_height >= ihdr_min,
        "pre_fix_repro_screenshot_full_page_taller_than_viewport: \
         IHDR height {ihdr_height} < min {ihdr_min}"
    );
}

/// `live_screenshot_full_page_md5_differs_from_viewport`:
///
/// Capture the same tall page with and without `--full-page`; assert the two
/// PNGs differ (i.e., `--full-page` produced something taller than the viewport).
///
/// The session-59 regression mode: both captures were byte-identical because the
/// `fullPage:true` flag was silently dropped.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_screenshot_full_page_md5_differs_from_viewport() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!(
            "live_screenshot_full_page_md5_differs_from_viewport: \
             set FF_RDP_LIVE_TESTS=1 to run"
        );
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!(
            "live_screenshot_full_page_md5_differs_from_viewport: \
             Firefox not available — skipping"
        );
        return;
    };

    let ff_args = || base_args(ff.port());

    // Navigate to the tall page.
    let nav = Command::new(ff_rdp_bin())
        .args(ff_args())
        .args(["navigate", TALL_PAGE_URL])
        .output()
        .expect("navigate to tall page");
    assert!(
        nav.status.success(),
        "live_screenshot_full_page_md5_differs_from_viewport: \
         navigate failed — {}",
        String::from_utf8_lossy(&nav.stderr)
    );

    // Viewport capture.
    let viewport_out = Command::new(ff_rdp_bin())
        .args(ff_args())
        .args(["screenshot", "--base64"])
        .output()
        .expect("viewport screenshot");
    assert!(
        viewport_out.status.success(),
        "live_screenshot_full_page_md5_differs_from_viewport: \
         viewport screenshot failed — {}",
        String::from_utf8_lossy(&viewport_out.stderr)
    );

    // Full-page capture.
    let full_page_out = Command::new(ff_rdp_bin())
        .args(ff_args())
        .args(["screenshot", "--full-page", "--base64"])
        .output()
        .expect("full-page screenshot");
    assert!(
        full_page_out.status.success(),
        "live_screenshot_full_page_md5_differs_from_viewport: \
         full-page screenshot failed — {}",
        String::from_utf8_lossy(&full_page_out.stderr)
    );

    let vp_b64 = {
        let r = parse_results_json(&viewport_out);
        r["base64"].as_str().expect("viewport base64").to_owned()
    };
    let fp_b64 = {
        let r = parse_results_json(&full_page_out);
        r["base64"].as_str().expect("full-page base64").to_owned()
    };

    let vp_bytes = base64::engine::general_purpose::STANDARD
        .decode(&vp_b64)
        .expect("valid viewport base64");
    let fp_bytes = base64::engine::general_purpose::STANDARD
        .decode(&fp_b64)
        .expect("valid full-page base64");

    let vp_height = png_height_from_bytes(&vp_bytes).unwrap_or(0);
    let fp_height = png_height_from_bytes(&fp_bytes).unwrap_or(0);

    assert!(
        fp_height > vp_height,
        "live_screenshot_full_page_md5_differs_from_viewport: \
         full-page height ({fp_height}) must exceed viewport height ({vp_height}); \
         the two captures appear identical — `--full-page` flag was not honoured"
    );

    // Also assert byte-inequality (the regression detection from session-59).
    assert_ne!(
        vp_bytes, fp_bytes,
        "live_screenshot_full_page_md5_differs_from_viewport: \
         full-page and viewport captures are byte-identical — \
         `--full-page` flag was silently dropped"
    );
}
