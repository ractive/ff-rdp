//! iter-83 AC: `live_styles_applied_returns_real_rules`.
//!
//! Loads a fixture page with three CSS rules (one UA reset stub + two real
//! author rules) and asserts that `styles 'p' --applied` returns at least 2
//! rules with non-empty `properties`.
//!
//! This tests Theme E (iter-83): the narrowed `is_ua_reset_stub` filter must
//! keep author rules with non-empty properties while still dropping the UA
//! `*, ::after, ::before {}` stubs.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live live_styles_applied -- --nocapture

use std::process::Command;

use crate::common::{LiveFirefox, base_args, ff_rdp_bin};

/// Fixture page: a `<p>` element with an explicit UA-reset stub plus two author
/// CSS rules.  The stub `*, ::after, ::before{}` exercises the `is_ua_reset_stub`
/// filter against an actual matching selector; the two author rules must survive.
const FIXTURE_HTML: &str = "data:text/html;charset=utf-8,\
<!DOCTYPE html><html><head>\
<style>*, ::after, ::before{}</style>\
<style>p{color:red;font-size:16px}</style>\
<style>p{margin:0;padding:0}</style>\
</head><body><p>test</p></body></html>";

/// `live_styles_applied_returns_real_rules` (iter-83 AC):
///
/// Navigate to a fixture page with two `<style>` blocks that both target `p`,
/// run `styles p --applied`, and assert:
///   - The command exits 0.
///   - At least 2 results have non-empty `properties`.
///
/// This validates Theme E: the dedupe filter keeps real author rules while
/// discarding UA-reset stubs.
///
/// Gated on `FF_RDP_LIVE_TESTS=1`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_styles_applied_returns_real_rules() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_styles_applied_returns_real_rules: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_styles_applied_returns_real_rules: Firefox not available — skipping");
        return;
    };

    // Navigate to fixture.
    let nav = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["navigate", FIXTURE_HTML, "--wait-strategy", "readystate"])
        .output()
        .expect("ff-rdp navigate");
    assert!(
        nav.status.success(),
        "live_styles_applied_returns_real_rules: navigate failed — {}",
        String::from_utf8_lossy(&nav.stderr)
    );

    // Run styles p --applied.
    let out = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["styles", "p", "--applied"])
        .output()
        .expect("ff-rdp styles p --applied");
    assert!(
        out.status.success(),
        "live_styles_applied_returns_real_rules: styles failed — stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    let json: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
        panic!(
            "live_styles_applied_returns_real_rules: output is not valid JSON: {e}\n\
             stdout={stdout}\nstderr={}",
            String::from_utf8_lossy(&out.stderr)
        )
    });

    let results = json["results"]
        .as_array()
        .expect("results must be an array");

    // Count rules with non-empty properties.
    let rules_with_props: Vec<_> = results
        .iter()
        .filter(|r| {
            r.get("properties")
                .and_then(|p| p.as_array())
                .is_some_and(|arr| !arr.is_empty())
        })
        .collect();

    assert!(
        rules_with_props.len() >= 2,
        "live_styles_applied_returns_real_rules: expected at least 2 rules with non-empty \
         properties, got {}; full results={results:?}",
        rules_with_props.len()
    );

    // Theme E regression guard: no empty-properties rule with a UA-reset selector
    // pattern should appear in results.  If the filter regresses, such rules
    // would reappear and this assertion catches it.
    let ua_reset_leak: Vec<_> = results
        .iter()
        .filter(|r| {
            let props_empty = r
                .get("properties")
                .and_then(|p| p.as_array())
                .is_some_and(Vec::is_empty);
            let sel = r.get("selector").and_then(|s| s.as_str()).unwrap_or("");
            props_empty
                && (sel.contains("*, ::after, ::before")
                    || sel.contains("*,::after,::before")
                    || (sel.contains('*') && sel.contains("::before") && sel.contains("::after")))
        })
        .collect();
    assert!(
        ua_reset_leak.is_empty(),
        "live_styles_applied_returns_real_rules: UA-reset stub leaked into results: {ua_reset_leak:?}"
    );

    eprintln!(
        "live_styles_applied_returns_real_rules: PASS — {} rules with properties \
         (out of {} total)",
        rules_with_props.len(),
        results.len()
    );
}
