/// Live test for Theme L (iter-84): `cookies list` surfaces cookies set via
/// `Set-Cookie` response headers on the navigation that just completed,
/// even when Firefox has not yet flushed them to the StorageActor.
///
/// httpbin.org/cookies/set?probe=1 redirects and sets a cookie via header.
///
/// AC: live_cookies_set_cookie_header — results contains {name:"probe",value:"1"}
///     within one invocation of `cookies list`
use crate::common::{live_network_tests_enabled, live_tests_enabled};
use std::process::Command;

fn ff_rdp_bin() -> String {
    env!("CARGO_BIN_EXE_ff-rdp").to_string()
}

/// Theme L: cookies set via `Set-Cookie` response header (httpbin redirect)
/// appear in `cookies list` output immediately after navigation.
///
/// Pre-condition: Firefox running with `--start-debugger-server 6000`.
/// Post-condition: cookie `probe=1` present in results.
#[test]
#[ignore = "requires FF_RDP_LIVE_TESTS=1 and running Firefox"]
fn live_cookies_set_cookie_header_visible_after_navigate() {
    if !live_tests_enabled() || !live_network_tests_enabled() {
        return;
    }

    // This URL sets a cookie via Set-Cookie header in a redirect.
    let nav = Command::new(ff_rdp_bin())
        .args(["navigate", "https://httpbin.org/cookies/set?probe=1"])
        .output()
        .expect("ff-rdp navigate failed");
    assert!(
        nav.status.success(),
        "navigate failed: {}",
        String::from_utf8_lossy(&nav.stderr)
    );

    let out = Command::new(ff_rdp_bin())
        .args(["cookies", "list"])
        .output()
        .expect("ff-rdp cookies list failed");

    assert!(
        out.status.success(),
        "cookies list failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    let json: serde_json::Value =
        serde_json::from_str(&stdout).expect("cookies list output is not valid JSON");

    let results = json["results"].as_array().expect("results is not array");

    let probe_cookie = results
        .iter()
        .find(|c| c.get("name").and_then(|v| v.as_str()) == Some("probe"));

    assert!(
        probe_cookie.is_some(),
        "Theme L regression: cookie 'probe' not found in cookies list after \
         navigating to httpbin.org/cookies/set?probe=1 — Set-Cookie header \
         cookie may not have been flushed to StorageActor yet"
    );

    if let Some(cookie) = probe_cookie {
        assert_eq!(
            cookie.get("value").and_then(|v| v.as_str()),
            Some("1"),
            "cookie 'probe' has wrong value: {cookie:?}"
        );
    }
}
