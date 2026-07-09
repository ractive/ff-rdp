/// Live tests for cascade command against real sites.
///
/// Theme A (iter-84): cascade returns applied rules even when Firefox omits
/// the `type` field on CSS rules from external stylesheets (e.g. css.gg).
///
/// Theme A (iter-85): cascade returns applied rules when Firefox sends
/// `type: 100` (CSSStyleRule) for ordinary author rules — previously these
/// were dropped, returning an empty `rules[]`.
///
/// AC: live_cascade_real_site — returns ≥1 rule for `<a>` on css.gg
/// AC: live_cascade_real_site_cli — returns ≥1 rule for `h1` on tennis-sepp.ch
use crate::common::{live_network_tests_enabled, live_tests_enabled};
use std::process::Command;

/// Returns the path to the built ff-rdp binary.
fn ff_rdp_bin() -> String {
    env!("CARGO_BIN_EXE_ff-rdp").to_string()
}

/// iter-85 Theme A: `cascade h1 --prop color` on tennis-sepp.ch must return
/// ≥1 rule even when Firefox 151 sends `type: 100` (CSSStyleRule) for ordinary
/// author rules (the old type-based guard would have dropped them).
///
/// Pre-condition: Firefox running with `--start-debugger-server 6000`; network access.
/// Post-condition: `results[0].rules | length >= 1`.
#[test]
#[ignore = "requires FF_RDP_LIVE_TESTS=1 and FF_RDP_LIVE_NETWORK_TESTS=1"]
fn live_cascade_real_site_cli() {
    if !live_tests_enabled() || !live_network_tests_enabled() {
        return;
    }

    let nav = Command::new(ff_rdp_bin())
        .args(["navigate", "https://tennis-sepp.ch"])
        .output()
        .expect("ff-rdp navigate failed");
    assert!(
        nav.status.success(),
        "navigate https://tennis-sepp.ch failed: {}",
        String::from_utf8_lossy(&nav.stderr)
    );

    let out = Command::new(ff_rdp_bin())
        .args(["cascade", "h1", "--prop", "color"])
        .output()
        .expect("ff-rdp cascade failed");

    let stdout = String::from_utf8_lossy(&out.stdout);
    let json: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("cascade output is not valid JSON");

    let rules = json["results"][0]["rules"]
        .as_array()
        .expect("results[0].rules is not an array");
    assert!(
        !rules.is_empty(),
        "iter-85 Theme A regression: cascade returned 0 rules for h1 on tennis-sepp.ch \
         (Firefox may be sending type:100 rules that are still being dropped)"
    );
}

/// Theme A (iter-84): `cascade` returns ≥1 rule for `a` elements on a real page that
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
