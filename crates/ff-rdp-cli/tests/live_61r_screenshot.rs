//! Live tests for iter-61r Theme B — screenshot --full-page correctness.
//!
//! Verifies that `ff-rdp screenshot --full-page` captures the full scroll
//! height of a tall synthetic page rather than clipping to the viewport.
//!
//! # Running
//!
//! Requires Firefox and the ff-rdp binary.  Gates on `FF_RDP_LIVE_TESTS=1`.
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live_61r_screenshot -- --nocapture

#[path = "common/mod.rs"]
mod common;

use std::process::{Command, Output};

use base64::Engine as _;
use common::{LiveFirefox, base_args, ff_rdp_bin};

fn parse_json(output: &Output) -> serde_json::Value {
    let s = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(s.trim()).unwrap_or_else(|e| {
        panic!(
            "stdout is not valid JSON: {e}\nstdout={s}\nstderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

/// Extract PNG height from the IHDR chunk (bytes 20–23, big-endian).
fn png_height_from_bytes(data: &[u8]) -> Option<u32> {
    if data.len() < 24 {
        return None;
    }
    Some(u32::from_be_bytes(data[20..24].try_into().ok()?))
}

/// Navigate to `about:blank`, inject a 5 000 px tall document body via
/// `eval`, take a `--full-page` screenshot, and assert the PNG height
/// ≥ 4 900 px.
///
/// Using `about:blank` + `eval` (rather than a data-URL) avoids any external
/// network dependency and any data-URL encoding complexity, and is
/// deterministic across runs.
///
/// AC: `live_screenshot_full_page` — PNG height ≥ 4 900 px on a 5 000 px
/// synthetic page.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_screenshot_full_page() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_screenshot_full_page: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_screenshot_full_page: Firefox not available — skipping");
        return;
    };

    let ff_args = || base_args(ff.port());

    // Use a tall page injected via JS eval to avoid data-URL encoding complexity.
    // We navigate to about:blank first, then set body height via eval before
    // taking the screenshot.
    let data_url = "about:blank";

    // Navigate to about:blank.
    let nav = Command::new(ff_rdp_bin())
        .args(ff_args())
        .args(["navigate", data_url])
        .output()
        .expect("navigate to about:blank");

    assert!(
        nav.status.success(),
        "live_screenshot_full_page: navigate failed — {}",
        String::from_utf8_lossy(&nav.stderr)
    );

    // Inject a 5 000 px tall document body via eval.
    let setup = Command::new(ff_rdp_bin())
        .args(ff_args())
        .args([
            "eval",
            "document.body.style.height='5000px'; document.body.style.background='#f00'; 'ok'",
        ])
        .output()
        .expect("eval body height");

    assert!(
        setup.status.success(),
        "live_screenshot_full_page: body setup eval failed — {}",
        String::from_utf8_lossy(&setup.stderr)
    );

    // Take a full-page screenshot in base64 mode so we don't need a temp file.
    let out = Command::new(ff_rdp_bin())
        .args(ff_args())
        .args(["screenshot", "--full-page", "--base64"])
        .output()
        .expect("screenshot --full-page --base64");

    assert!(
        out.status.success(),
        "live_screenshot_full_page: screenshot exited non-zero — stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json = parse_json(&out);

    // The PNG height reported by ff-rdp must be ≥ 4 900 px.
    let height = json["results"]["height"]
        .as_u64()
        .expect("results.height must be a number");

    assert!(
        height >= 4_900,
        "live_screenshot_full_page: PNG height must be ≥ 4900 px on a 5000 px page; got {height}"
    );

    // Cross-check: decode the base64 and verify from the PNG IHDR header.
    let b64 = json["results"]["base64"]
        .as_str()
        .expect("results.base64 must be present when --base64 is used");
    let png_bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .expect("valid base64");
    let ihdr_height =
        png_height_from_bytes(&png_bytes).expect("PNG IHDR height must be extractable");
    assert!(
        ihdr_height >= 4_900,
        "live_screenshot_full_page: PNG IHDR height {ihdr_height} < 4900"
    );
}
