/// Live test for Theme A (iter-84): cascade command returns applied rules
/// even when Firefox omits the `type` field on CSS rules from external
/// stylesheets (e.g. css.gg icon library).
///
/// AC: live_cascade_real_site — returns ≥1 rule for `<a>` on css.gg
use std::process::Command;

/// Returns the path to the built ff-rdp binary.
fn ff_rdp_bin() -> String {
    env!("CARGO_BIN_EXE_ff-rdp").to_string()
}

fn live_tests_enabled() -> bool {
    std::env::var("FF_RDP_LIVE_TESTS").as_deref() == Ok("1")
}

fn live_network_tests_enabled() -> bool {
    std::env::var("FF_RDP_LIVE_NETWORK_TESTS").as_deref() == Ok("1")
}

/// Theme A: `cascade` returns ≥1 rule for `a` elements on a real page that
/// uses external stylesheets whose rules may lack the `type` field.
///
/// Pre-condition: Firefox running with `--start-debugger-server 6000`.
/// Post-condition: `results` array has ≥1 entry.
#[test]
#[ignore = "requires FF_RDP_LIVE_TESTS=1 and running Firefox"]
fn live_cascade_real_site_returns_rules_for_a_element() {
    if !live_tests_enabled() || !live_network_tests_enabled() {
        return;
    }

    // Navigate to a page known to have external stylesheets.
    let nav = Command::new(ff_rdp_bin())
        .args(["navigate", "https://css.gg/"])
        .output()
        .expect("ff-rdp navigate failed");
    assert!(
        nav.status.success(),
        "navigate failed: {}",
        String::from_utf8_lossy(&nav.stderr)
    );

    let out = Command::new(ff_rdp_bin())
        .args(["cascade", "--selector", "a"])
        .output()
        .expect("ff-rdp cascade failed");

    let stdout = String::from_utf8_lossy(&out.stdout);
    let json: serde_json::Value =
        serde_json::from_str(&stdout).expect("cascade output is not valid JSON");

    let results = json["results"].as_array().expect("results is not an array");
    assert!(
        !results.is_empty(),
        "Theme A regression: cascade returned 0 rules for `a` on css.gg \
         (Firefox may be omitting `type` on external-stylesheet rules)"
    );
}
