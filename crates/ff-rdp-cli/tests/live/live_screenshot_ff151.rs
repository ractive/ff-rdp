/// Live tests for Theme B (iter-84/85): screenshot works on Firefox 151+ where
/// the two-step capture protocol (`prepareCapture` + `capture`) is required
/// and `screenshotActor` may or may not be present in `getRoot`.
///
/// AC: live_screenshot_ff151 — PNG bytes > 0 on current Firefox version
/// AC: live_screenshot_ff151_cli — file exists + PNG magic bytes present
use crate::common::live_tests_enabled;
use std::process::Command;

fn ff_rdp_bin() -> String {
    env!("CARGO_BIN_EXE_ff-rdp").to_string()
}

/// Theme B: `screenshot` produces a non-empty PNG on any supported Firefox
/// version including 151+ where the getRoot response may not include
/// `screenshotActor` directly.
///
/// Pre-condition: Firefox running with `--start-debugger-server 6000`.
/// Post-condition: stdout starts with PNG magic bytes (PNG header).
#[test]
#[ignore = "requires FF_RDP_LIVE_TESTS=1 and running Firefox"]
fn live_screenshot_ff151_produces_valid_png() {
    if !live_tests_enabled() {
        return;
    }

    let out = Command::new(ff_rdp_bin())
        .args(["screenshot", "--output", "-"])
        .output()
        .expect("ff-rdp screenshot failed");

    assert!(
        out.status.success(),
        "screenshot failed (Theme B regression — screenshotActor not found): {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // PNG magic: 137 80 78 71 13 10 26 10
    assert!(
        out.stdout
            .starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]),
        "screenshot output is not a valid PNG (first 8 bytes: {:?})",
        &out.stdout[..out.stdout.len().min(8)]
    );
    assert!(
        out.stdout.len() > 1000,
        "screenshot PNG too small: {} bytes",
        out.stdout.len()
    );
}

/// Theme B (iter-85 AC): `ff-rdp screenshot -o /tmp/x.png` on example.com
/// produces a file that exists and starts with valid PNG magic bytes.
///
/// Uses the screenshot_via_target fallback path when `screenshotActor` is
/// absent from `getRoot` (Firefox 151+).
///
/// Pre-condition: Firefox running, navigated to example.com.
/// Post-condition: /tmp/x.png exists with PNG magic `\x89PNG\r\n\x1a\n`.
#[test]
#[ignore = "requires FF_RDP_LIVE_TESTS=1 and running Firefox"]
fn live_screenshot_ff151_cli() {
    if !live_tests_enabled() {
        return;
    }

    let out_path = std::env::temp_dir().join("live_screenshot_ff151_cli_test.png");

    let result = Command::new(ff_rdp_bin())
        .arg("screenshot")
        .arg("-o")
        .arg(&out_path)
        .output()
        .expect("ff-rdp screenshot failed");

    assert!(
        result.status.success(),
        "screenshot -o {} failed (Theme B): {}",
        out_path.display(),
        String::from_utf8_lossy(&result.stderr)
    );

    let bytes = std::fs::read(&out_path)
        .unwrap_or_else(|e| panic!("screenshot file not found at {}: {e}", out_path.display()));

    // PNG magic: 137 80 78 71 13 10 26 10
    assert!(
        bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]),
        "screenshot file is not a valid PNG (first 8 bytes: {:?})",
        &bytes[..bytes.len().min(8)]
    );
    assert!(
        bytes.len() > 1000,
        "screenshot PNG too small: {} bytes",
        bytes.len()
    );

    // Clean up.
    let _ = std::fs::remove_file(&out_path);
}
