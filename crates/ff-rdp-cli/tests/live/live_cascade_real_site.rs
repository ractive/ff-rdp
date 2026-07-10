/// Live tests for cascade command against real sites.
///
/// Theme A (iter-84): cascade returns applied rules even when Firefox omits
/// the `type` field on CSS rules from external stylesheets (e.g. css.gg).
///
/// Theme A (iter-85): cascade returns applied rules when Firefox sends
/// `type: 100` (CSSStyleRule) for ordinary author rules — previously these
/// were dropped, returning an empty `rules[]`.
///
/// iter-114 Theme B: ported to the self-launch harness. Only one real-site
/// network smoke is kept (`live_cascade_real_site_cli` against
/// tennis-sepp.ch) — see its doc comment for the retirement rationale on the
/// former css.gg test.
///
/// AC: live_cascade_real_site_cli — returns ≥1 rule for `h1` on tennis-sepp.ch
use crate::common::{LiveFirefox, base_args, ff_rdp_bin, live_network_tests_enabled};
use std::process::Command;

/// iter-85 Theme A: `cascade h1 --prop color` on tennis-sepp.ch must return
/// ≥1 rule even when Firefox 151 sends `type: 100` (CSSStyleRule) for ordinary
/// author rules (the old type-based guard would have dropped them).
///
/// This is the one deliberately-kept real-site network smoke (iter-114 Theme
/// B): it exercises cascade end-to-end against a real, non-synthetic
/// stylesheet shape rather than a hand-built fixture. Ported to the
/// self-launch harness but still gated on both `FF_RDP_LIVE_TESTS=1` and
/// `FF_RDP_LIVE_NETWORK_TESTS=1`.
///
/// Post-condition: `results[0].rules | length >= 1`.
#[test]
#[ignore = "requires FF_RDP_LIVE_TESTS=1 and FF_RDP_LIVE_NETWORK_TESTS=1"]
fn live_cascade_real_site_cli() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() || !live_network_tests_enabled() {
        eprintln!(
            "live_cascade_real_site_cli: set FF_RDP_LIVE_TESTS=1 and FF_RDP_LIVE_NETWORK_TESTS=1 to run"
        );
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_cascade_real_site_cli: Firefox not available — skipping");
        return;
    };

    let nav = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["navigate", "https://tennis-sepp.ch"])
        .output()
        .expect("ff-rdp navigate failed");
    assert!(
        nav.status.success(),
        "navigate https://tennis-sepp.ch failed: {}",
        String::from_utf8_lossy(&nav.stderr)
    );

    let out = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["cascade", "h1", "--prop", "color"])
        .output()
        .expect("ff-rdp cascade failed");
    assert!(
        out.status.success(),
        "cascade failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

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

    eprintln!(
        "live_cascade_real_site_cli: PASS — {} rule(s) for h1 on tennis-sepp.ch",
        rules.len()
    );
}

// `live_cascade_real_site_returns_rules_for_a_element` (former css.gg smoke,
// Theme A iter-84) was retired in iter-114 Theme B rather than ported: its
// coverage — cascade returns ≥1 rule for an element on a real site with
// external stylesheets — is subsumed by the kept `live_cascade_real_site_cli`
// smoke above plus the local (non-network) cascade suites `live_cascade`,
// `live_95_cascade_computed_agreement`, and
// `live_cascade_explains_pico_dialog`, which exercise the same
// external/multi-stylesheet and `type`-field code paths against
// deterministic fixtures.
