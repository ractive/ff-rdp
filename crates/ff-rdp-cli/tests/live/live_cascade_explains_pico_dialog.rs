//! iter-81 AC: `live_cascade_explains_pico_dialog`.
//!
//! Navigates Firefox to a fixture page that loads two stylesheets:
//!   - a "pico-like" base stylesheet declaring `dialog { display: block }`
//!   - a "site" stylesheet declaring `dialog#lightbox { display: flex }`
//!
//! Then asserts that `ff-rdp cascade 'dialog#lightbox' --prop display`
//! returns *at least* two rules with distinct stylesheets and that the
//! winning rule's `value` matches the computed value reported by
//! `ff-rdp computed 'dialog#lightbox' --prop display`.
//!
//! # Running
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli --test live live_cascade_explains_pico_dialog -- --nocapture

use std::process::Command;

use crate::common::{LiveFirefox, base_args, ff_rdp_bin};
use serde_json::Value;

/// Two-stylesheet fixture: the first `<style>` sets a low-specificity
/// `dialog` rule, the second a higher-specificity `dialog#lightbox`
/// override.  Firefox sees two distinct stylesheets (one per `<style>`
/// element), so the cascade output must list both.
const FIXTURE_HTML: &str = "data:text/html;charset=utf-8,\
<!DOCTYPE html><html><head>\
<style id='pico'>dialog{display:block}</style>\
<style id='site'>dialog%23lightbox{display:flex}</style>\
</head><body>\
<dialog id='lightbox' open>cascade test</dialog>\
</body></html>";

#[test]
#[ignore = "requires Firefox + FF_RDP_LIVE_TESTS=1"]
fn live_cascade_explains_pico_dialog() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_cascade_explains_pico_dialog: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }
    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_cascade_explains_pico_dialog: Firefox not available — skipping");
        return;
    };

    let mut args = base_args(ff.port());
    // iter-110 Theme B(a): data: URLs require --allow-unsafe-urls.
    args.extend([
        "navigate".into(),
        "--allow-unsafe-urls".into(),
        FIXTURE_HTML.into(),
    ]);
    let out = Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("ff-rdp navigate");
    assert!(
        out.status.success(),
        "navigate failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // 1) cascade — must list ≥ 2 rules with distinct stylesheets.
    let mut args = base_args(ff.port());
    args.extend([
        "cascade".into(),
        "dialog#lightbox".into(),
        "--prop".into(),
        "display".into(),
    ]);
    let out = Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("ff-rdp cascade");
    assert!(
        out.status.success(),
        "cascade failed: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let cascade: Value = serde_json::from_slice(&out.stdout).expect("cascade JSON");
    let entry = &cascade["results"][0];
    assert_eq!(entry["property"], "display");
    let rules = entry["rules"].as_array().expect("rules array");
    assert!(
        rules.len() >= 2,
        "expected ≥ 2 cascade rules, got {}: {entry}",
        rules.len()
    );

    // The two rules must come from distinct stylesheets (or distinct
    // lines within the document — for inline <style> blocks Firefox
    // reports the host document URL, but the line numbers differ).
    let mut keys = std::collections::HashSet::new();
    for r in rules {
        keys.insert(format!(
            "{}:{}",
            r["stylesheet"].as_str().unwrap_or(""),
            r["line"]
        ));
    }
    assert!(keys.len() >= 2, "expected distinct sources, got: {keys:?}");

    // The winning rule's `value` must equal the cascade's `computed` field.
    let computed_in_cascade = entry["computed"].as_str().expect("computed str");
    let winner = rules
        .iter()
        .find(|r| r["winner"] == Value::Bool(true))
        .expect("winner row");
    assert_eq!(
        winner["value"].as_str().unwrap_or(""),
        computed_in_cascade,
        "winner value must equal cascade.computed"
    );

    // 2) computed — winner's value must match the resolved computed value.
    let mut args = base_args(ff.port());
    args.extend([
        "computed".into(),
        "dialog#lightbox".into(),
        "--prop".into(),
        "display".into(),
    ]);
    let out = Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("ff-rdp computed");
    assert!(out.status.success(), "computed failed");
    let computed: Value = serde_json::from_slice(&out.stdout).expect("computed JSON");
    // `computed` returns a results array; each entry has a `computed` map
    // with the requested property name as key.
    let display = computed["results"][0]["computed"]["display"]
        .as_str()
        .expect("display value in computed output");
    assert_eq!(
        display, computed_in_cascade,
        "computed.display must match the cascade winner"
    );
}
