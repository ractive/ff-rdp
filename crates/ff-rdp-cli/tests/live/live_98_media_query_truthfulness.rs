//! Live tests for iter-98 — media-query truthfulness.
//!
//! Theme A: `responsive` must never present a media-query-untruthful viewport
//! state without flagging it. Over RDP the emulation is layout-only, so the
//! self-check (`media_query_check`) reports whether the page's media queries
//! actually flipped to the requested width, and `--strict` turns a mismatch
//! into a non-zero exit.
//!
//! Theme B: `cascade`'s winner flag must respect the live `@media` context and
//! agree with `computed`.
//!
//! ACs (see kb/iterations/iteration-98-media-query-truthfulness.md):
//!   - pre_fix_repro_responsive_media_queries_do_not_flip
//!   - live_responsive_self_check_reports_mismatch
//!   - pre_fix_repro_cascade_winner_ignores_media_context
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live live_98_media_query_truthfulness -- --nocapture

use std::process::Command;

use crate::common::live_tests_enabled;
use crate::common::{LiveFirefox, base_args, ff_rdp_bin};

/// A data-URL fixture whose `#probe` element is styled narrow (390px) by
/// default and 980px inside `@media (min-width: 1024px)`. Mirrors the
/// field-report scenario (shell-main reporting 980px at a claimed 390px
/// viewport).
const FIXTURE_HTML: &str = "data:text/html;charset=utf-8,\
<!DOCTYPE html><html><head><style>\
#probe{width:390px}\
@media (min-width: 1024px){#probe{width:980px}}\
</style></head><body><div id='probe'>x</div></body></html>";

fn navigate(port: u16, url: &str) {
    let nav = Command::new(ff_rdp_bin())
        .args(base_args(port))
        // data: URLs require --allow-unsafe-urls (off by default).
        .args(["navigate", "--allow-unsafe-urls", url])
        .output()
        .expect("ff-rdp navigate");
    assert!(
        nav.status.success(),
        "navigate failed: {}",
        String::from_utf8_lossy(&nav.stderr)
    );
}

/// `pre_fix_repro_responsive_media_queries_do_not_flip`:
///
/// Run `responsive #probe --widths 390`. Post-fix the envelope must carry a
/// `media_query_check` object for the 390px breakpoint. Because RDP emulation
/// is layout-only, the real (headless-default ~1024px+) viewport does not flip
/// media queries to 390px, so `media_query_check.matches` must be `false` and a
/// warning must be present — the media-query-untruthful state is flagged rather
/// than presented silently.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn pre_fix_repro_responsive_media_queries_do_not_flip() {
    if !live_tests_enabled() {
        eprintln!("pre_fix_repro_responsive_media_queries_do_not_flip: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("Firefox not available — skipping");
        return;
    };
    navigate(ff.port(), FIXTURE_HTML);

    let out = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["responsive", "#probe", "--widths", "390"])
        .output()
        .expect("ff-rdp responsive");
    assert!(
        out.status.success(),
        "responsive (non-strict) must exit 0; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    let json: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("responsive output not JSON: {e}\n{stdout}"));

    let bp = &json["results"]["breakpoints"][0];
    assert_eq!(bp["width"], 390);
    let mq = &bp["media_query_check"];
    assert_eq!(
        mq["requested"], 390,
        "media_query_check must echo requested width"
    );
    assert_eq!(
        mq["matches"], false,
        "layout-only emulation: (width: 390px) must NOT match the physical viewport: {json}"
    );

    // The warning must be present so the untruthful state is flagged.
    let warnings = json["results"]["warnings"]
        .as_array()
        .expect("warnings array present on mismatch");
    assert!(
        warnings.iter().any(|w| w
            .as_str()
            .is_some_and(|s| s.contains("media queries did not flip"))),
        "a media-query mismatch warning must be present: {warnings:?}"
    );
}

/// `live_responsive_self_check_reports_mismatch`:
///
/// With the emulation layout-only (the live default over RDP), the envelope
/// contains `media_query_check` with `matches == false` and `--strict` exits
/// non-zero.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_responsive_self_check_reports_mismatch() {
    if !live_tests_enabled() {
        eprintln!("live_responsive_self_check_reports_mismatch: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("Firefox not available — skipping");
        return;
    };
    navigate(ff.port(), FIXTURE_HTML);

    let out = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["responsive", "#probe", "--widths", "390", "--strict"])
        .output()
        .expect("ff-rdp responsive --strict");

    // --strict must exit non-zero on the layout-only media-query mismatch.
    assert!(
        !out.status.success(),
        "responsive --strict must exit non-zero when media queries do not flip; \
         stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    // The envelope is still emitted on stdout with matches == false.
    let stdout = String::from_utf8_lossy(&out.stdout);
    let json: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("responsive --strict output not JSON: {e}\n{stdout}"));
    assert_eq!(
        json["results"]["breakpoints"][0]["media_query_check"]["matches"], false,
        "strict-mode envelope must still report matches == false: {json}"
    );
}

/// `pre_fix_repro_cascade_winner_ignores_media_context`:
///
/// At a ≥1024px viewport the `(min-width: 1024px)` rule for `#probe`'s `width`
/// must be the winner and its value must equal `computed`'s answer for the
/// property. Pre-fix the winner algorithm ignored media context; post-fix the
/// winner is media-aware and agrees with computed (`winner_verified: true`).
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn pre_fix_repro_cascade_winner_ignores_media_context() {
    if !live_tests_enabled() {
        eprintln!("pre_fix_repro_cascade_winner_ignores_media_context: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("Firefox not available — skipping");
        return;
    };
    navigate(ff.port(), FIXTURE_HTML);

    let out = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["cascade", "#probe", "--prop", "width"])
        .output()
        .expect("ff-rdp cascade");
    assert!(
        out.status.success(),
        "cascade must exit 0; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    let json: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("cascade output not JSON: {e}\n{stdout}"));

    let entry = &json["results"][0];
    let computed = entry["computed"].as_str().unwrap_or("");
    // Headless Firefox defaults to a wide viewport, so the (min-width:1024px)
    // media block is active and computed width is 980px.
    assert_eq!(
        computed, "980px",
        "at a wide viewport computed width must be the media override: {json}"
    );

    let rules = entry["rules"].as_array().expect("rules array");
    let winner = rules
        .iter()
        .find(|r| r["winner"] == true)
        .expect("a winner rule must be flagged");
    assert_eq!(
        winner["value"], "980px",
        "the media-active override must win, matching computed: {json}"
    );
    // The winner value agrees with computed → verified true.
    assert_eq!(
        entry["winner_verified"], true,
        "winner must be verified against computed: {json}"
    );
}
