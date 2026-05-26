//! Live test: `live_screenshot_unchanged_after_shim` (iter-78 AC1).
//!
//! Verifies that the `ScreenshotArgsExt` shim introduced in iter-77 (Theme A)
//! — which sends `browsingContextID`, `snapshotScale`, and `rect` even though
//! those fields are not declared in the published Firefox spec dict — still
//! produces a valid PNG round-trip against a live Firefox instance.
//!
//! Because no pre-iter-77 baseline PNG exists, the test interprets the AC as:
//! the screenshot must be a structurally valid PNG (IHDR present) with
//! sensible dimensions (height ≥ 1 100 px on a 1 200 px synthetic body,
//! width ≥ 1 px).
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live_screenshot_shim -- --nocapture

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

/// Extract PNG width from the IHDR chunk (bytes 16–19, big-endian).
fn png_width_from_bytes(data: &[u8]) -> Option<u32> {
    if data.len() < 24 {
        return None;
    }
    Some(u32::from_be_bytes(data[16..20].try_into().ok()?))
}

/// Extract PNG height from the IHDR chunk (bytes 20–23, big-endian).
fn png_height_from_bytes(data: &[u8]) -> Option<u32> {
    if data.len() < 24 {
        return None;
    }
    Some(u32::from_be_bytes(data[20..24].try_into().ok()?))
}

/// `live_screenshot_unchanged_after_shim`:
/// Navigate to `about:blank`, inject a 1 200 px tall body with a known
/// background colour via `eval`, then take a `--full-page --base64`
/// screenshot and assert:
///
/// - The command exits with code 0.
/// - `results.base64` decodes to valid PNG bytes (starts with the 8-byte PNG
///   magic and contains the IHDR signature at offset 12).
/// - PNG IHDR height ≥ 1 100 px (allowing headroom for DPR / rounding).
/// - PNG IHDR width ≥ 1 px.
///
/// Gated on `FF_RDP_LIVE_TESTS=1`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_screenshot_unchanged_after_shim() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_screenshot_unchanged_after_shim: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_screenshot_unchanged_after_shim: Firefox not available — skipping");
        return;
    };

    let ff_args = || base_args(ff.port());

    // Navigate to about:blank.
    let nav = Command::new(ff_rdp_bin())
        .args(ff_args())
        .args(["navigate", "about:blank"])
        .output()
        .expect("navigate to about:blank");

    assert!(
        nav.status.success(),
        "live_screenshot_unchanged_after_shim: navigate failed — {}",
        String::from_utf8_lossy(&nav.stderr)
    );

    // Inject a 1 200 px tall blue body.
    let setup = Command::new(ff_rdp_bin())
        .args(ff_args())
        .args([
            "eval",
            "document.body.style.height='1200px'; \
             document.body.style.background='#0066cc'; \
             'ok'",
        ])
        .output()
        .expect("eval body setup");

    assert!(
        setup.status.success(),
        "live_screenshot_unchanged_after_shim: body setup eval failed — {}",
        String::from_utf8_lossy(&setup.stderr)
    );

    // Take a full-page screenshot in base64 mode.
    // The ScreenshotArgsExt shim sends browsingContextID / snapshotScale / rect
    // fields that are not in the spec dict; if Firefox rejects them the command
    // exits non-zero.
    let out = Command::new(ff_rdp_bin())
        .args(ff_args())
        .args(["screenshot", "--full-page", "--base64"])
        .output()
        .expect("screenshot --full-page --base64");

    assert!(
        out.status.success(),
        "live_screenshot_unchanged_after_shim: screenshot exited non-zero \
         (ScreenshotArgsExt shim may have broken the request) — stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json = parse_json(&out);

    // Decode the base64 blob and verify PNG structure via IHDR.
    let b64 = json["results"]["base64"]
        .as_str()
        .expect("results.base64 must be present with --base64");
    let png_bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .expect("results.base64 must be valid base64");

    // PNG magic: 8 bytes, then 4-byte chunk length, 4-byte "IHDR".
    assert!(
        png_bytes.len() >= 24,
        "live_screenshot_unchanged_after_shim: PNG blob too short ({} bytes)",
        png_bytes.len()
    );
    assert_eq!(
        &png_bytes[..8],
        &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A],
        "live_screenshot_unchanged_after_shim: PNG magic bytes missing — not a valid PNG"
    );
    assert_eq!(
        &png_bytes[12..16],
        b"IHDR",
        "live_screenshot_unchanged_after_shim: IHDR chunk missing"
    );

    let width = png_width_from_bytes(&png_bytes).expect("PNG IHDR width must be extractable");
    let height = png_height_from_bytes(&png_bytes).expect("PNG IHDR height must be extractable");

    assert!(
        width >= 1,
        "live_screenshot_unchanged_after_shim: PNG IHDR width {width} < 1"
    );
    assert!(
        height >= 1_100,
        "live_screenshot_unchanged_after_shim: PNG IHDR height {height} < 1100 \
         (expected ≥ 1100 px on a 1200 px synthetic body)"
    );

    eprintln!(
        "live_screenshot_unchanged_after_shim: PASS — PNG {width}×{height} px, \
         {} bytes",
        png_bytes.len()
    );
}

/// `live_screenshot_no_args_on_firefox_151` (iter-82 AC):
///
/// Runs `ff-rdp screenshot -o <tmp>.png` with no extra flags and asserts
/// that:
///   - The command exits with code 0.
///   - The output file exists and is non-empty.
///   - The file begins with the 8-byte PNG magic (`\x89PNG\r\n\x1a\n`).
///
/// This covers the path through the iter-78 shim that the earlier baseline
/// (`live_screenshot_unchanged_after_shim`) did not exercise: the shim must
/// work without `--full-page` and `--base64` flags, writing a file to disk
/// via `-o`.
///
/// Gated on `FF_RDP_LIVE_TESTS=1`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_screenshot_no_args_on_firefox_151() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_screenshot_no_args_on_firefox_151: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_screenshot_no_args_on_firefox_151: Firefox not available — skipping");
        return;
    };

    // Navigate to a simple page so there is content to capture.
    let nav = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["navigate", "about:blank"])
        .output()
        .expect("navigate to about:blank");
    assert!(
        nav.status.success(),
        "live_screenshot_no_args_on_firefox_151: navigate failed — {}",
        String::from_utf8_lossy(&nav.stderr)
    );

    // Write screenshot to a temp file.
    let tmp = std::env::temp_dir().join("live_screenshot_no_args_on_firefox_151.png");

    let out = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .arg("screenshot")
        .arg("-o")
        .arg(&tmp)
        .output()
        .expect("ff-rdp screenshot -o <tmp>");

    assert!(
        out.status.success(),
        "live_screenshot_no_args_on_firefox_151: screenshot exited non-zero — stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    assert!(
        tmp.exists(),
        "live_screenshot_no_args_on_firefox_151: output file not created at {tmp:?}"
    );

    let bytes = std::fs::read(&tmp)
        .unwrap_or_else(|e| panic!("live_screenshot_no_args_on_firefox_151: read {tmp:?}: {e}"));

    assert!(
        !bytes.is_empty(),
        "live_screenshot_no_args_on_firefox_151: output file is empty"
    );

    assert_eq!(
        &bytes[..8.min(bytes.len())],
        &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A],
        "live_screenshot_no_args_on_firefox_151: file does not start with PNG magic bytes"
    );

    eprintln!(
        "live_screenshot_no_args_on_firefox_151: PASS — PNG {} bytes",
        bytes.len()
    );
}
