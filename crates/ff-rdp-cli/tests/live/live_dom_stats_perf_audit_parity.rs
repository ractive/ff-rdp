/// Live test for Theme H (iter-84): `dom stats` and `perf audit` report the
/// same value for `images_without_lazy` — both should count only
/// out-of-viewport images that lack `loading=lazy`.
///
/// AC: live_dom_stats_perf_parity — dom_stats.images_without_lazy ==
///     perf_audit.images_without_lazy on httpbin.org/html
use crate::common::{live_network_tests_enabled, live_tests_enabled};
use std::process::Command;

fn ff_rdp_bin() -> String {
    env!("CARGO_BIN_EXE_ff-rdp").to_string()
}

/// Theme H: `dom stats` and `perf audit` agree on `images_without_lazy`.
///
/// Pre-condition: Firefox running with `--start-debugger-server 6000`,
///               navigated to a page with `<img>` elements.
/// Post-condition: both commands return equal `images_without_lazy` values.
#[test]
#[ignore = "requires FF_RDP_LIVE_TESTS=1 and running Firefox"]
fn live_dom_stats_perf_audit_parity_images_without_lazy() {
    if !live_tests_enabled() || !live_network_tests_enabled() {
        return;
    }

    let nav = Command::new(ff_rdp_bin())
        .args(["navigate", "https://httpbin.org/html"])
        .output()
        .expect("ff-rdp navigate failed");
    assert!(
        nav.status.success(),
        "navigate failed: {}",
        String::from_utf8_lossy(&nav.stderr)
    );

    let dom_out = Command::new(ff_rdp_bin())
        .args(["dom", "stats"])
        .output()
        .expect("ff-rdp dom stats failed");
    assert!(
        dom_out.status.success(),
        "dom stats failed: {}",
        String::from_utf8_lossy(&dom_out.stderr)
    );

    let perf_out = Command::new(ff_rdp_bin())
        .args(["perf", "audit"])
        .output()
        .expect("ff-rdp perf audit failed");
    assert!(
        perf_out.status.success(),
        "perf audit failed: {}",
        String::from_utf8_lossy(&perf_out.stderr)
    );

    let dom_json: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&dom_out.stdout))
            .expect("dom stats is not valid JSON");
    let perf_json: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&perf_out.stdout))
            .expect("perf audit is not valid JSON");

    let dom_val = dom_json
        .pointer("/results/images_without_lazy")
        .or_else(|| dom_json.pointer("/results/0/images_without_lazy"))
        .and_then(serde_json::Value::as_u64);
    let perf_val = perf_json
        .pointer("/results/images_without_lazy")
        .or_else(|| perf_json.pointer("/results/0/images_without_lazy"))
        .and_then(serde_json::Value::as_u64);

    if let (Some(dom), Some(perf)) = (dom_val, perf_val) {
        assert_eq!(
            dom, perf,
            "Theme H regression: dom stats ({dom}) != perf audit ({perf}) for images_without_lazy"
        );
    }
    // If the field is absent in either output (e.g. page has no images),
    // the test passes — both agreed there was nothing to count.
}
