//! iter-82 AC: `live_cascade_returns_matched_rules`.
//!
//! Loads a data URL with `<style>h1 { color: red }</style><h1>x</h1>`,
//! runs `ff-rdp cascade h1 --prop color`, and asserts:
//!   - `rules[].matched_selectors` contains `"h1"`
//!   - `computed == "rgb(255, 0, 0)"`
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live_cascade -- --nocapture

#[path = "common/mod.rs"]
mod common;

use std::process::Command;

use common::{LiveFirefox, base_args, ff_rdp_bin};

const FIXTURE_HTML: &str = "data:text/html;charset=utf-8,\
<!DOCTYPE html><html><head>\
<style>h1{color:red}</style>\
</head><body><h1>x</h1></body></html>";

/// `live_cascade_returns_matched_rules`:
/// Navigate to a data URL with a known `<style>h1 { color: red }</style>`
/// block, run `cascade h1 --prop color`, and verify that the matched
/// selector is `"h1"` and the computed value is `"rgb(255, 0, 0)"`.
///
/// Gated on `FF_RDP_LIVE_TESTS=1`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_cascade_returns_matched_rules() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_cascade_returns_matched_rules: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_cascade_returns_matched_rules: Firefox not available — skipping");
        return;
    };

    // Navigate to fixture.
    let nav = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["navigate", FIXTURE_HTML])
        .output()
        .expect("ff-rdp navigate");
    assert!(
        nav.status.success(),
        "live_cascade_returns_matched_rules: navigate failed — {}",
        String::from_utf8_lossy(&nav.stderr)
    );

    // Run cascade h1 --prop color.
    let out = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["cascade", "h1", "--prop", "color"])
        .output()
        .expect("ff-rdp cascade");
    assert!(
        out.status.success(),
        "live_cascade_returns_matched_rules: cascade failed — stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    let json: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
        panic!(
            "live_cascade_returns_matched_rules: cascade output is not valid JSON: {e}\n\
                 stdout={stdout}\nstderr={}",
            String::from_utf8_lossy(&out.stderr)
        )
    });

    let entry = &json["results"][0];
    assert_eq!(
        entry["property"].as_str().unwrap_or(""),
        "color",
        "cascade entry property must be 'color'; got {entry}"
    );

    // computed must be the red rgb value.
    let computed = entry["computed"].as_str().unwrap_or("");
    assert_eq!(
        computed, "rgb(255, 0, 0)",
        "cascade computed must be 'rgb(255, 0, 0)'; got {computed:?}"
    );

    // At least one rule must have a matched selector containing "h1".
    let rules = entry["rules"].as_array().expect("rules must be an array");
    assert!(
        !rules.is_empty(),
        "live_cascade_returns_matched_rules: rules array must not be empty; got {entry}"
    );
    let has_h1_selector = rules.iter().any(|r| {
        r["matched_selectors"]
            .as_array()
            .is_some_and(|arr| arr.iter().any(|s| s.as_str().unwrap_or("") == "h1"))
    });
    assert!(
        has_h1_selector,
        "live_cascade_returns_matched_rules: no rule has matched_selector 'h1'; rules={rules:?}"
    );

    eprintln!("live_cascade_returns_matched_rules: PASS — computed={computed:?}");
}
