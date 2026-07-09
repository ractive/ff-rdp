/// Live test for Theme J (iter-84): `a11y contrast` detects failures on the
/// WAI bad-example page (https://www.w3.org/WAI/demos/bad/) which is the
/// canonical reference for low-contrast text.
///
/// The fix widens element detection to include containers where all children
/// are inline elements (span, a, b, etc.) — not just leaf text nodes.
///
/// AC: live_a11y_contrast_wai_bad — aa_fail ≥ 1 on WAI bad demo page
use crate::common::{live_network_tests_enabled, live_tests_enabled};
use std::process::Command;

fn ff_rdp_bin() -> String {
    env!("CARGO_BIN_EXE_ff-rdp").to_string()
}

/// Theme J: `a11y contrast` with `--fail-only` returns at least one failure
/// on the WAI bad-example page which deliberately contains low-contrast text.
///
/// Pre-condition: Firefox running with `--start-debugger-server 6000`.
/// Post-condition: `summary.aa_fail` ≥ 1.
#[test]
#[ignore = "requires FF_RDP_LIVE_TESTS=1 and running Firefox"]
fn live_a11y_contrast_wai_bad_detects_failures() {
    if !live_tests_enabled() || !live_network_tests_enabled() {
        return;
    }

    let nav = Command::new(ff_rdp_bin())
        .args(["navigate", "https://www.w3.org/WAI/demos/bad/before/"])
        .output()
        .expect("ff-rdp navigate failed");
    assert!(
        nav.status.success(),
        "navigate failed: {}",
        String::from_utf8_lossy(&nav.stderr)
    );

    let out = Command::new(ff_rdp_bin())
        .args(["a11y", "contrast", "--fail-only"])
        .output()
        .expect("ff-rdp a11y contrast failed");

    assert!(
        out.status.success(),
        "a11y contrast failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    let json: serde_json::Value =
        serde_json::from_str(&stdout).expect("a11y contrast output is not valid JSON");

    let aa_fail = json
        .pointer("/meta/summary/aa_fail")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);

    assert!(
        aa_fail >= 1,
        "Theme J regression: WAI bad demo reported 0 AA failures \
         (contrast detection too narrow — may have missed inline-child containers)"
    );

    let total = json["total"].as_u64().unwrap_or(0);
    assert!(
        total >= 1,
        "Theme J: --fail-only returned 0 results (aa_fail={aa_fail})"
    );
}
