//! iter-135 — `screenshot` must work again on Firefox 153.
//!
//! # The bug
//!
//! Every live RDP capture on Firefox 153.0.3 failed with
//! `screenshotActor capture response missing 'data' field`.  The reply shape had
//! not changed: Firefox returned `{"value":{"data":null,…,"messages":[…]}}`
//! because ff-rdp omitted `snapshotScale` whenever it equalled `1.0`.  The
//! server has no default for that field — it hands the value straight to
//! `drawSnapshot`, so `undefined` produced a `NaN`-sized canvas.
//!
//! The omission was latent on Firefox 149–152 (`screenshotActor.capture` failed
//! earlier, at actor-module load, and ff-rdp fell back to `drawSnapshot`).
//! Firefox 153 fixed that load failure (Bug 2043900), so the request finally
//! reached the renderer and the omission became fatal.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live live_135 -- --include-ignored

use std::process::Command;

use crate::common::{LiveFirefox, base_args, ff_rdp_bin};
use base64::Engine as _;

/// A page that comfortably exceeds the headless viewport (~683 px at 1× DPR).
const TALL_PAGE_URL: &str = "data:text/html,<html><body style=\"height:4000px;background:linear-gradient(to bottom,red,blue)\">iter-135</body></html>";

/// A short page that fits in the viewport.
const SHORT_PAGE_URL: &str =
    "data:text/html,<html><body style=\"background:#0a0\">iter-135 short</body></html>";

/// A page so tall that the renderer is guaranteed to give up, used to force a
/// capture failure while the session is unambiguously headless.
const ABSURD_PAGE_URL: &str = "data:text/html,<html><body style=\"height:200000px;background:red\">iter-135 absurd</body></html>";

/// The hint iter-135 removed.  It fired on every capture failure, including for
/// sessions that were already headless.
const MISLEADING_HINT: &str = "relaunch with: ff-rdp launch --headless";

fn skip(test: &str) -> bool {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("{test}: set FF_RDP_LIVE_TESTS=1 to run");
        return true;
    }
    false
}

fn navigate(port: u16, url: &str, test: &str) {
    let nav = Command::new(ff_rdp_bin())
        .args(base_args(port))
        .args(["navigate", "--allow-unsafe-urls", url])
        .output()
        .expect("run navigate");
    assert!(
        nav.status.success(),
        "{test}: navigate failed — {}",
        String::from_utf8_lossy(&nav.stderr)
    );
}

fn parse_results(out: &std::process::Output, test: &str) -> serde_json::Value {
    let s = String::from_utf8_lossy(&out.stdout);
    let top: serde_json::Value = serde_json::from_str(s.trim())
        .unwrap_or_else(|e| panic!("{test}: stdout is not JSON: {e}\nstdout={s}"));
    top["results"].clone()
}

/// Decode `results.base64` and return `(width, height)` from the PNG IHDR,
/// asserting the PNG signature is intact.
fn png_dimensions(results: &serde_json::Value, test: &str) -> (u32, u32) {
    let b64 = results["base64"]
        .as_str()
        .unwrap_or_else(|| panic!("{test}: results.base64 missing"));
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .unwrap_or_else(|e| panic!("{test}: results.base64 is not valid base64: {e}"));
    assert!(
        bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]),
        "{test}: payload does not start with the PNG magic bytes"
    );
    assert!(bytes.len() >= 24, "{test}: PNG truncated before IHDR");
    let width = u32::from_be_bytes(bytes[16..20].try_into().expect("4 bytes"));
    let height = u32::from_be_bytes(bytes[20..24].try_into().expect("4 bytes"));
    (width, height)
}

/// `live_135_screenshot_ff153_capture`:
///
/// A plain `ff-rdp screenshot -o <path>` against headless Firefox 153 writes a
/// real PNG — magic bytes present, both dimensions non-zero — and does not
/// report `missing 'data' field`.
///
/// Pre-fix: exited non-zero with
/// `screenshot: screenshotActor.capture failed (invalid packet: screenshotActor
/// capture response missing 'data' field) — screenshots require headless mode…`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_135_screenshot_ff153_capture() {
    const TEST: &str = "live_135_screenshot_ff153_capture";
    if skip(TEST) {
        return;
    }
    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("{TEST}: Firefox not available — skipping");
        return;
    };

    navigate(ff.port(), SHORT_PAGE_URL, TEST);

    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("iter135.png");

    let out = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["screenshot", "-o"])
        .arg(&path)
        .output()
        .expect("run screenshot");

    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        out.status.success(),
        "{TEST}: screenshot exited non-zero — stdout={stdout} stderr={stderr}"
    );
    assert!(
        !stdout.contains("missing 'data' field") && !stderr.contains("missing 'data' field"),
        "{TEST}: the iter-135 failure mode is back — stdout={stdout}"
    );

    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{TEST}: read {path:?}: {e}"));
    assert!(
        bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]),
        "{TEST}: written file is not a PNG"
    );
    assert!(bytes.len() >= 24, "{TEST}: PNG truncated before IHDR");
    let width = u32::from_be_bytes(bytes[16..20].try_into().expect("4 bytes"));
    let height = u32::from_be_bytes(bytes[20..24].try_into().expect("4 bytes"));
    assert!(
        width > 0 && height > 0,
        "{TEST}: PNG has zero dimensions ({width}×{height})"
    );
}

/// `live_135_screenshot_full_page_taller`:
///
/// On a 4 000 px page, the `--full-page` PNG is strictly taller than the plain
/// viewport PNG captured from the same page.
///
/// This is the same invariant iter-92 established; it is re-asserted here
/// because on Firefox 153 the viewport capture goes through
/// `screenshotActor.capture` while `--full-page` goes through the
/// `drawSnapshot` path — the fix has to hold for both.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_135_screenshot_full_page_taller() {
    const TEST: &str = "live_135_screenshot_full_page_taller";
    if skip(TEST) {
        return;
    }
    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("{TEST}: Firefox not available — skipping");
        return;
    };

    navigate(ff.port(), TALL_PAGE_URL, TEST);

    let capture = |extra: &[&str]| -> serde_json::Value {
        let out = Command::new(ff_rdp_bin())
            .args(base_args(ff.port()))
            .args(["screenshot", "--base64"])
            .args(extra)
            .output()
            .expect("run screenshot");
        assert!(
            out.status.success(),
            "{TEST}: screenshot {extra:?} exited non-zero — {}",
            String::from_utf8_lossy(&out.stderr)
        );
        parse_results(&out, TEST)
    };

    let (_, viewport_h) = png_dimensions(&capture(&[]), TEST);
    let (_, full_h) = png_dimensions(&capture(&["--full-page"]), TEST);

    assert!(
        full_h > viewport_h,
        "{TEST}: --full-page height {full_h} is not greater than viewport height {viewport_h}"
    );
}

/// `live_135_screenshot_error_not_misleading`:
///
/// Force a capture failure against an unambiguously headless Firefox (a
/// 200 000 px page defeats the renderer) and assert the error says what
/// actually happened rather than telling the user to relaunch headless — which
/// they already are.
///
/// If a machine is beefy enough to render the page anyway the invariant still
/// holds and is still checked: no output from the command may carry the hint.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_135_screenshot_error_not_misleading() {
    const TEST: &str = "live_135_screenshot_error_not_misleading";
    if skip(TEST) {
        return;
    }
    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("{TEST}: Firefox not available — skipping");
        return;
    };

    navigate(ff.port(), ABSURD_PAGE_URL, TEST);

    let out = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["screenshot", "--full-page", "--base64"])
        .output()
        .expect("run screenshot");

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    assert!(
        !combined.contains(MISLEADING_HINT),
        "{TEST}: the misleading headless hint is back — output={combined}"
    );
    assert!(
        !combined.contains("screenshot actor not found"),
        "{TEST}: error falsely claims the screenshot actor is missing — output={combined}"
    );

    if out.status.success() {
        eprintln!("{TEST}: renderer survived the 200 000 px page; hint-absence still asserted");
    } else {
        assert!(
            combined.contains("rendered no image for this capture"),
            "{TEST}: failure must state what actually happened — output={combined}"
        );
    }
}
