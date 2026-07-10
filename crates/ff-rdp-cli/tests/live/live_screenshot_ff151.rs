/// Live tests for Theme B (iter-84/85): screenshot works on Firefox 151+ where
/// the two-step capture protocol (`prepareCapture` + `capture`) is required
/// and `screenshotActor` may or may not be present in `getRoot`.
///
/// Both tests self-launch headless Firefox on a random port and navigate to a
/// small deterministic `data:` URL before capturing, so the captured page
/// content is fixed rather than whatever tab Firefox happens to have open by
/// default — this also makes the "PNG size > 1000 bytes" assertion meaningful
/// rather than accidental.
///
/// AC: live_screenshot_ff151 — base64-decoded PNG bytes > 1000 on current Firefox version
/// AC: live_screenshot_ff151_cli — file exists + PNG magic bytes present
use std::process::Command;

use base64::Engine as _;

use crate::common::{LiveFirefox, base_args, ff_rdp_bin, live_tests_enabled};

/// A small deterministic fixture with enough visible content to produce a
/// meaningfully sized PNG capture.
const FIXTURE_HTML: &str = "data:text/html;charset=utf-8,\
<!DOCTYPE html><html><head><style>\
body{margin:0}\
div{width:400px;height:300px;background:linear-gradient(45deg,red,blue)}\
</style></head><body><div></div><h1>live_screenshot_ff151</h1></body></html>";

fn navigate(port: u16) {
    let nav = Command::new(ff_rdp_bin())
        .args(base_args(port))
        .args(["navigate", "--allow-unsafe-urls", FIXTURE_HTML])
        .output()
        .expect("ff-rdp navigate");
    assert!(
        nav.status.success(),
        "navigate failed: {}",
        String::from_utf8_lossy(&nav.stderr)
    );
}

/// Theme B: `screenshot` produces a non-empty PNG on any supported Firefox
/// version including 151+ where the getRoot response may not include
/// `screenshotActor` directly.
///
/// Self-launches headless Firefox on a random port and navigates to a fixed
/// deterministic fixture page before capturing.
///
/// Uses `--base64` rather than the legacy `--output -` (which — per the
/// current CLI's `-o/--output <PATH>` semantics, see `ff-rdp screenshot
/// --help` — writes a literal file named `-` rather than streaming raw PNG
/// bytes to stdout; there is no more "write PNG to stdout" mode) and decodes
/// the returned base64 PNG data, keeping this test's original intent (PNG
/// magic bytes + minimum size) while exercising a JSON-output code path
/// distinct from [`live_screenshot_ff151_cli`]'s `-o <path>` capture.
/// Post-condition: base64-decoded PNG data starts with PNG magic bytes (PNG header).
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_screenshot_ff151_produces_valid_png() {
    if !live_tests_enabled() {
        eprintln!("live_screenshot_ff151_produces_valid_png: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_screenshot_ff151_produces_valid_png: Firefox not available — skipping");
        return;
    };

    navigate(ff.port());

    let out = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["screenshot", "--base64"])
        .output()
        .expect("ff-rdp screenshot failed");

    assert!(
        out.status.success(),
        "screenshot failed (Theme B regression — screenshotActor not found): {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    let json: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("screenshot --base64 output not JSON: {e}\n{stdout}"));
    let b64 = json["results"]["base64"]
        .as_str()
        .unwrap_or_else(|| panic!("screenshot --base64 output missing results.base64: {json}"));
    let png_bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .unwrap_or_else(|e| panic!("screenshot --base64 output is not valid base64: {e}"));

    // PNG magic: 137 80 78 71 13 10 26 10
    assert!(
        png_bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]),
        "screenshot output is not a valid PNG (first 8 bytes: {:?})",
        &png_bytes[..png_bytes.len().min(8)]
    );
    assert!(
        png_bytes.len() > 1000,
        "screenshot PNG too small: {} bytes",
        png_bytes.len()
    );
}

/// Theme B (iter-85 AC): `ff-rdp screenshot -o <path>` on a fixed deterministic
/// fixture produces a file that exists and starts with valid PNG magic bytes.
///
/// Uses the screenshot_via_target fallback path when `screenshotActor` is
/// absent from `getRoot` (Firefox 151+).
///
/// Self-launches headless Firefox on a random port and navigates to a fixed
/// deterministic fixture page before capturing.
/// Post-condition: output file exists with PNG magic `\x89PNG\r\n\x1a\n`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_screenshot_ff151_cli() {
    if !live_tests_enabled() {
        eprintln!("live_screenshot_ff151_cli: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_screenshot_ff151_cli: Firefox not available — skipping");
        return;
    };

    navigate(ff.port());

    let out_path = std::env::temp_dir().join("live_screenshot_ff151_cli_test.png");

    let result = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
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
