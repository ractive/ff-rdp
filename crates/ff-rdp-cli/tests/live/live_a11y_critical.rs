//! iter-80 Theme E AC: `a11y_critical_filters_to_violations`.
//!
//! Navigates to a fixture data: URL with a known WCAG violation (an `<img>`
//! without `alt`) and asserts that `ff-rdp a11y --critical` returns exactly
//! the offending node. Then re-navigates to a clean page (alt present) and
//! asserts an empty result set.
//!
//! # Running
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli --test live live_a11y_critical -- --nocapture

use std::process::Command;

use crate::common::{LiveFirefox, base_args, ff_rdp_bin};

fn run_a11y_critical(port: u16) -> serde_json::Value {
    let mut args = base_args(port);
    args.extend(["a11y".into(), "--critical".into()]);
    let out = Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("ff-rdp a11y --critical");
    assert!(
        out.status.success(),
        "a11y --critical failed: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("a11y --critical output not valid JSON: {e}\n{stdout}"))
}

fn navigate(port: u16, url: &str) {
    let mut args = base_args(port);
    args.extend(["navigate".into(), url.into()]);
    let out = Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("ff-rdp navigate");
    assert!(
        out.status.success(),
        "navigate failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
#[ignore = "requires Firefox + FF_RDP_LIVE_TESTS=1"]
fn a11y_critical_filters_to_violations() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("a11y_critical_filters_to_violations: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }
    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("a11y_critical_filters_to_violations: Firefox not available — skipping");
        return;
    };

    // Page with a known WCAG violation: <img> with no alt attribute.
    let bad = "data:text/html,<title>bad</title><img id=\"hero\" src=\"x.png\">";
    navigate(ff.port(), bad);
    let json = run_a11y_critical(ff.port());
    let results = json["results"]
        .as_array()
        .unwrap_or_else(|| panic!("expected results array; got {json}"));
    assert_eq!(
        results.len(),
        1,
        "expected exactly one violation; got {json}"
    );
    assert_eq!(results[0]["violation"], "missing-alt");
    assert_eq!(results[0]["role"], "img");

    // Clean page: <img> has alt, no other violators.
    let good = "data:text/html,<title>good</title><img alt=\"hero\" src=\"x.png\">";
    navigate(ff.port(), good);
    let json = run_a11y_critical(ff.port());
    let results = json["results"]
        .as_array()
        .unwrap_or_else(|| panic!("expected results array; got {json}"));
    assert!(
        results.is_empty(),
        "clean page must produce zero violations; got {json}"
    );
}
