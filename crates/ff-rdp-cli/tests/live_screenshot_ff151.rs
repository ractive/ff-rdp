/// Live test for Theme B (iter-84): screenshot works on Firefox 151+ where
/// the two-step capture protocol (`prepareCapture` + `capture`) is required
/// and `screenshotActor` must be discoverable from `getRoot`.
///
/// AC: live_screenshot_ff151 — PNG bytes > 0 on current Firefox version
use std::process::Command;

fn ff_rdp_bin() -> String {
    env!("CARGO_BIN_EXE_ff-rdp").to_string()
}

fn live_tests_enabled() -> bool {
    std::env::var("FF_RDP_LIVE_TESTS").as_deref() == Ok("1")
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
