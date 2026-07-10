//! Live test for iter-76b Theme A — bulk fallback does not poison the stream.
//!
//! Verifies that running `ff-rdp screenshot --bulk` followed by
//! `ff-rdp eval` against the same Firefox instance both succeed.
//! This is a regression test for the stream-poison bug introduced in iter-76
//! where the bulk-recv path consumed the first byte of the JSON response
//! without putting it back, causing the next `recv_from` to misparse the frame.
//!
//! # Running
//!
//! Requires Firefox and the ff-rdp binary.  Gates on `FF_RDP_LIVE_TESTS=1`.
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live live_screenshot_bulk_fallback -- --nocapture

use crate::common::{LiveFirefox, base_args, ff_rdp_bin};

/// A small deterministic fixture with a non-empty `document.title`, so the
/// eval-after-bulk-screenshot assertion (title must be non-empty) is
/// verified against fixed content rather than depending on a real external
/// site (`https://example.com`).
const FIXTURE_HTML: &str = "data:text/html;charset=utf-8,<!DOCTYPE html><html><head><title>x</title></head><body><h1>live_screenshot_bulk_fallback</h1></body></html>";

/// AC: `live_screenshot_bulk_fallback_then_eval` — `--bulk` screenshot then
/// `eval` both succeed against the same Firefox instance (regression for the
/// stream-poison bug).
///
/// The stream-poison bug: `try_bulk_screenshot` used `send_capture_request`
/// (one-way) then `recv_bulk_with_handler`.  Firefox responded with a JSON
/// frame (which is length-prefixed: `<digits>:<json>`); the old code consumed
/// the first byte — the leading digit of the length prefix — via `read_exact`,
/// returned `BulkPacketUnexpected`, then fell through to
/// `ScreenshotActor::capture` which reads from the same transport.  The
/// subsequent `recv_from` would see the remainder of the length prefix without
/// its first digit and fail to parse a valid `<digits>:` header.  With the fix
/// (`try_two_step_screenshot` always calls `ScreenshotActor::capture` directly),
/// the stream stays aligned.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_screenshot_bulk_fallback_then_eval() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_screenshot_bulk_fallback_then_eval: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_screenshot_bulk_fallback_then_eval: Firefox not available — skipping");
        return;
    };

    // Navigate to a simple, deterministic local page (no real network access
    // required — this file has no `live_network_tests_enabled()` gate to drop).
    let nav = std::process::Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["navigate", "--allow-unsafe-urls", FIXTURE_HTML])
        .output()
        .expect("navigate command should run");
    assert!(
        nav.status.success(),
        "navigate failed: {}",
        String::from_utf8_lossy(&nav.stderr)
    );

    // Take a --bulk screenshot.  `--bulk` is now a boolean flag (the bulk path
    // is a no-op that always falls through to JSON capture) — the output path
    // is a separate `-o/--output` argument, so this should succeed cleanly.
    let tmp_path = std::env::temp_dir().join("test-bulk-76b.png");
    let screenshot = std::process::Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["screenshot", "--bulk", "-o", tmp_path.to_str().unwrap()])
        .output()
        .expect("screenshot command should run");
    assert!(
        screenshot.status.success(),
        "screenshot --bulk failed: {}\nstdout: {}",
        String::from_utf8_lossy(&screenshot.stderr),
        String::from_utf8_lossy(&screenshot.stdout),
    );
    let _ = std::fs::remove_file(&tmp_path);

    // Immediately run eval on the same Firefox — must succeed (stream not poisoned).
    let eval = std::process::Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["eval", "document.title"])
        .output()
        .expect("eval command should run");
    assert!(
        eval.status.success(),
        "eval after --bulk screenshot failed: {}\nstdout: {}",
        String::from_utf8_lossy(&eval.stderr),
        String::from_utf8_lossy(&eval.stdout),
    );

    // The title must be non-empty.
    let stdout = String::from_utf8_lossy(&eval.stdout);
    assert!(
        !stdout.trim().is_empty(),
        "eval returned empty output — expected a page title"
    );
    // The JSON output must contain a non-null, non-empty result. `eval`'s
    // current envelope carries the evaluated value directly under `results`
    // (e.g. `{"results": "x", ...}` for a string), not `results[0].result`.
    let json: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("eval stdout is not JSON: {e}\nstdout={stdout}"));
    let result = &json["results"];
    assert!(
        !result.is_null() && result != &serde_json::Value::String(String::new()),
        "eval result should be non-empty, got: {result}"
    );
}
