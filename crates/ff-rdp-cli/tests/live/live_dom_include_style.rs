//! iter-80 Theme D AC: `dom_include_style_attaches_computed_values`.
//!
//! Navigates to a fixture data: URL that contains `<p style="color:red">` and
//! asserts that `ff-rdp dom 'p' --include-style color` attaches a `style.color`
//! field with the resolved `rgb(255, 0, 0)` value.
//!
//! # Running
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli --test live live_dom_include_style -- --nocapture

use std::process::Command;

use crate::common::{LiveFirefox, base_args, ff_rdp_bin};

#[test]
#[ignore = "requires Firefox + FF_RDP_LIVE_TESTS=1"]
fn dom_include_style_attaches_computed_values() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("dom_include_style_attaches_computed_values: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }
    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("dom_include_style_attaches_computed_values: Firefox not available — skipping");
        return;
    };

    let url = "data:text/html,<title>include-style</title><p style=\"color:red\">a</p><p style=\"color:red\">b</p>";

    let mut args = base_args(ff.port());
    // iter-110 Theme B(a): data: URLs require --allow-unsafe-urls.
    args.extend(["navigate".into(), "--allow-unsafe-urls".into(), url.into()]);
    let out = Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("ff-rdp navigate");
    assert!(
        out.status.success(),
        "navigate failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let mut args = base_args(ff.port());
    args.extend([
        "dom".into(),
        "p".into(),
        "--include-style".into(),
        "color".into(),
    ]);
    let out = Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("ff-rdp dom --include-style");
    assert!(
        out.status.success(),
        "dom --include-style failed: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    let json: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("dom output not valid JSON: {e}\n{stdout}"));

    let arr = json["results"]
        .as_array()
        .unwrap_or_else(|| panic!("expected results array; got {json}"));
    assert!(arr.len() >= 2, "expected at least 2 matches; got {json}");
    for entry in arr {
        let color = entry["style"]["color"].as_str().unwrap_or_default();
        assert_eq!(
            color, "rgb(255, 0, 0)",
            "each match must carry style.color = rgb(255, 0, 0); got entry={entry}"
        );
    }
}
