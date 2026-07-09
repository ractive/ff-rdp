//! iter-82 AC: `live_cascade_returns_matched_rules`.
//! iter-83 AC: `live_cascade_returns_matched_rules_external_css`.
//!
//! Loads a data URL with `<style>h1 { color: red }</style><h1>x</h1>`,
//! runs `ff-rdp cascade h1 --prop color`, and asserts:
//!   - `rules[].selector` is the string `"h1"`
//!   - `computed == "rgb(255, 0, 0)"`
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live live_cascade -- --nocapture

use std::process::Command;

use crate::common::{LiveFirefox, base_args, ff_rdp_bin};
use base64::Engine as _;

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

    // At least one rule must have a selector of "h1".
    // CascadeEntry::to_json outputs `selector` (a string), not `matched_selectors`.
    let rules = entry["rules"].as_array().expect("rules must be an array");
    assert!(
        !rules.is_empty(),
        "live_cascade_returns_matched_rules: rules array must not be empty; got {entry}"
    );
    let has_h1_selector = rules
        .iter()
        .any(|r| r["selector"].as_str().unwrap_or("") == "h1");
    assert!(
        has_h1_selector,
        "live_cascade_returns_matched_rules: no rule has selector 'h1'; rules={rules:?}"
    );

    eprintln!("live_cascade_returns_matched_rules: PASS — computed={computed:?}");
}

/// `live_cascade_returns_matched_rules_external_css` (iter-83 AC):
///
/// Loads a data URL page that uses `<style>@import</style>` to pull in a
/// base64-encoded CSS blob that declares `h1 { color: red }`.  This exercises
/// the external-CSS code path through the `getApplied` parser (matchedSelectorIndexes
/// on entry vs. matchedSelectors on rule), which differs from the inline `<style>`
/// path tested by `live_cascade_returns_matched_rules`.
///
/// Asserts:
///   - At least one rule has a selector containing `"h1"`.
///   - `computed == "rgb(255, 0, 0)"`.
///
/// Gated on `FF_RDP_LIVE_TESTS=1`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_cascade_returns_matched_rules_external_css() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!(
            "live_cascade_returns_matched_rules_external_css: set FF_RDP_LIVE_TESTS=1 to run"
        );
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!(
            "live_cascade_returns_matched_rules_external_css: Firefox not available — skipping"
        );
        return;
    };

    // `h1{color:red}` base64-encoded to `aDF7Y29sb3I6cmVkfQ==`
    // data:text/css;base64,<base64(h1{color:red})>
    let css_b64 = base64::engine::general_purpose::STANDARD.encode(b"h1{color:red}");
    // Use @import inside a <style> block to simulate an external stylesheet.
    // data: URL @import is subject to same-origin policy in some browsers but
    // Firefox allows it within data: document contexts.
    let fixture = format!(
        "data:text/html;charset=utf-8,\
         <!DOCTYPE html><html><head>\
         <style>@import url('data:text/css;base64,{css_b64}');</style>\
         </head><body><h1>test</h1></body></html>"
    );

    // Navigate to fixture.
    let nav = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["navigate", &fixture, "--wait-strategy", "readystate"])
        .output()
        .expect("ff-rdp navigate");
    assert!(
        nav.status.success(),
        "live_cascade_returns_matched_rules_external_css: navigate must succeed so cascade \
         operates on the intended fixture — stderr={}",
        String::from_utf8_lossy(&nav.stderr)
    );

    // Brief sleep to allow @import to resolve.
    std::thread::sleep(std::time::Duration::from_millis(300));

    // Run cascade h1 --prop color.
    let out = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["cascade", "h1", "--prop", "color"])
        .output()
        .expect("ff-rdp cascade");
    assert!(
        out.status.success(),
        "live_cascade_returns_matched_rules_external_css: cascade failed — stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    let json: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
        panic!(
            "live_cascade_returns_matched_rules_external_css: cascade output is not valid JSON: \
             {e}\nstdout={stdout}\nstderr={}",
            String::from_utf8_lossy(&out.stderr)
        )
    });

    let entry = &json["results"][0];

    // computed must be the red rgb value.
    let computed = entry["computed"].as_str().unwrap_or("");
    assert_eq!(
        computed, "rgb(255, 0, 0)",
        "live_cascade_returns_matched_rules_external_css: computed must be 'rgb(255, 0, 0)'; \
         got {computed:?}; full entry={entry}"
    );

    // At least one rule must reference h1.
    let rules = entry["rules"].as_array().expect("rules must be an array");
    assert!(
        !rules.is_empty(),
        "live_cascade_returns_matched_rules_external_css: rules array must not be empty"
    );
    let has_h1_selector = rules
        .iter()
        .any(|r| r["selector"].as_str().unwrap_or("").contains("h1"));
    assert!(
        has_h1_selector,
        "live_cascade_returns_matched_rules_external_css: no rule has selector containing 'h1'; \
         rules={rules:?}"
    );

    eprintln!("live_cascade_returns_matched_rules_external_css: PASS — computed={computed:?}");
}
