//! Live tests for iter-103 — `emulate` (target-configuration actor).
//!
//! Each option is asserted by an in-page probe. Because emulation lives only
//! for the RDP connection that set it, the set + probe pair must run over the
//! *same* connection: these tests use the daemon path (no `--no-daemon`) so the
//! persistent daemon connection carries the configuration from the `emulate`
//! call to the following `eval`/`navigate`.
//!
//! ACs (see kb/iterations/iteration-103-target-configuration-cli.md):
//!   - live_emulate_color_scheme_dark
//!   - live_emulate_user_agent
//!   - live_emulate_dppx
//!   - live_emulate_js_disabled
//!   - live_emulate_offline
//!   - e2e_emulate_one_shot_lifetime_warning
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live live_103_emulate -- --nocapture

use std::process::Command;

use serde_json::Value;

use crate::common::live_tests_enabled;
use crate::common::{LiveFirefox, base_args, ff_rdp_bin};

/// Build daemon-path args (no `--no-daemon`): commands share the persistent
/// daemon connection, so emulation set by one command is visible to the next.
fn daemon_args(port: u16) -> Vec<String> {
    vec![
        "--host".to_owned(),
        "127.0.0.1".to_owned(),
        "--port".to_owned(),
        port.to_string(),
        "--timeout".to_owned(),
        "10000".to_owned(),
    ]
}

/// Stop the daemon for `port`, ignoring failures.
fn stop_daemon(port: u16) {
    let _ = Command::new(ff_rdp_bin())
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "daemon",
            "stop",
        ])
        .output();
}

/// Run `ff-rdp <args...>` and return the parsed JSON stdout, asserting success.
fn run_json(port: u16, extra: &[&str]) -> Value {
    let out = Command::new(ff_rdp_bin())
        .args(daemon_args(port))
        .args(extra)
        .output()
        .expect("ff-rdp command");
    assert!(
        out.status.success(),
        "command {extra:?} failed: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("output for {extra:?} not JSON: {e}\n{stdout}"))
}

/// Evaluate a JS expression over the daemon connection and return the result
/// as a JSON value (the eval envelope's `results` field).
fn eval(port: u16, expr: &str) -> Value {
    let json = run_json(port, &["eval", expr]);
    json["results"].clone()
}

/// Navigate over the daemon connection (data: URLs need --allow-unsafe-urls).
fn navigate(port: u16, url: &str) {
    let out = Command::new(ff_rdp_bin())
        .args(daemon_args(port))
        .args(["navigate", "--allow-unsafe-urls", url])
        .output()
        .expect("ff-rdp navigate");
    assert!(
        out.status.success(),
        "navigate failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// `live_emulate_color_scheme_dark`:
///
/// After `emulate --color-scheme dark`,
/// `matchMedia("(prefers-color-scheme: dark)").matches` is true; after
/// `emulate --reset` it reverts to the system default (false in headless).
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_emulate_color_scheme_dark() {
    if !live_tests_enabled() {
        eprintln!("live_emulate_color_scheme_dark: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("Firefox not available — skipping");
        return;
    };
    if ff.with_daemon().is_none() {
        eprintln!("daemon did not start — skipping");
        return;
    }
    let port = ff.port();

    navigate(port, "data:text/html,<h1>color scheme probe</h1>");

    // Baseline: system default is not dark in headless.
    let before = eval(port, "matchMedia('(prefers-color-scheme: dark)').matches");
    assert_eq!(
        before, false,
        "baseline prefers-color-scheme should not be dark: {before}"
    );

    // Apply dark simulation.
    let applied = run_json(port, &["emulate", "--color-scheme", "dark"]);
    assert_eq!(
        applied["results"]["applied"]["colorSchemeSimulation"], "dark",
        "envelope must echo the applied color scheme: {applied}"
    );
    // Daemon path: no one-shot lifetime warning.
    assert!(
        applied["results"].get("lifetime_warning").is_none(),
        "daemon-path envelope must NOT carry a lifetime warning: {applied}"
    );

    let after = eval(port, "matchMedia('(prefers-color-scheme: dark)').matches");
    assert_eq!(
        after, true,
        "after emulate --color-scheme dark the dark media query must match: {after}"
    );

    // Reset reverts to system default.
    run_json(port, &["emulate", "--reset"]);
    let reverted = eval(port, "matchMedia('(prefers-color-scheme: dark)').matches");
    assert_eq!(
        reverted, false,
        "after emulate --reset the dark media query must no longer match: {reverted}"
    );

    stop_daemon(port);
}

/// `live_emulate_user_agent`:
///
/// `navigator.userAgent` equals the override string after
/// `emulate --user-agent`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_emulate_user_agent() {
    if !live_tests_enabled() {
        eprintln!("live_emulate_user_agent: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("Firefox not available — skipping");
        return;
    };
    if ff.with_daemon().is_none() {
        eprintln!("daemon did not start — skipping");
        return;
    }
    let port = ff.port();

    navigate(port, "data:text/html,<h1>ua probe</h1>");

    run_json(port, &["emulate", "--user-agent", "ff-rdp-test/1.0"]);
    let ua = eval(port, "navigator.userAgent");
    assert_eq!(
        ua, "ff-rdp-test/1.0",
        "navigator.userAgent must equal the override: {ua}"
    );

    stop_daemon(port);
}

/// `live_emulate_dppx`:
///
/// `devicePixelRatio` equals the `--dppx` override.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_emulate_dppx() {
    if !live_tests_enabled() {
        eprintln!("live_emulate_dppx: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("Firefox not available — skipping");
        return;
    };
    if ff.with_daemon().is_none() {
        eprintln!("daemon did not start — skipping");
        return;
    }
    let port = ff.port();

    navigate(port, "data:text/html,<h1>dppx probe</h1>");

    run_json(port, &["emulate", "--dppx", "2"]);
    let dppx = eval(port, "window.devicePixelRatio");
    assert_eq!(
        dppx,
        serde_json::json!(2.0),
        "devicePixelRatio must equal the --dppx override: {dppx}"
    );

    stop_daemon(port);
}

/// `live_emulate_js_disabled`:
///
/// With `--js off` + reload, an inline script's DOM side-effect is absent;
/// with `--js on` + reload it returns. The fixture writes a marker attribute
/// from an inline `<script>` — present only when scripting runs.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_emulate_js_disabled() {
    if !live_tests_enabled() {
        eprintln!("live_emulate_js_disabled: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("Firefox not available — skipping");
        return;
    };
    if ff.with_daemon().is_none() {
        eprintln!("daemon did not start — skipping");
        return;
    }
    let port = ff.port();

    // Inline script sets a marker attribute on <html>. If JS is disabled the
    // attribute is never written.
    let fixture = "data:text/html,<html><body><h1>js probe</h1>\
<script>document.documentElement.setAttribute('data-js','ran')</script></body></html>";

    // Disable JS, then reload so the fixture loads without scripting.
    run_json(port, &["emulate", "--js", "off"]);
    navigate(port, fixture);
    // The probe itself runs via the console actor (server-side JS is still
    // available to the evaluator); it reads whether the *page's* inline script
    // executed by checking the marker attribute.
    let disabled_marker = eval(
        port,
        "document.documentElement.getAttribute('data-js') || 'absent'",
    );
    assert_eq!(
        disabled_marker, "absent",
        "with JS disabled the inline script must not run (marker absent): {disabled_marker}"
    );

    // Re-enable JS, reload, and confirm the inline script runs again.
    run_json(port, &["emulate", "--js", "on"]);
    navigate(port, fixture);
    let enabled_marker = eval(
        port,
        "document.documentElement.getAttribute('data-js') || 'absent'",
    );
    assert_eq!(
        enabled_marker, "ran",
        "with JS re-enabled the inline script must run (marker present): {enabled_marker}"
    );

    stop_daemon(port);
}

/// `live_emulate_offline`:
///
/// With `--offline on`, `navigator.onLine` reports false; restored after
/// `--offline off`. (A reload is applied so the offline state is reflected in
/// the document's `navigator.onLine`.)
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_emulate_offline() {
    if !live_tests_enabled() {
        eprintln!("live_emulate_offline: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("Firefox not available — skipping");
        return;
    };
    if ff.with_daemon().is_none() {
        eprintln!("daemon did not start — skipping");
        return;
    }
    let port = ff.port();

    navigate(port, "data:text/html,<h1>offline probe</h1>");

    let before = eval(port, "navigator.onLine");
    assert_eq!(before, true, "baseline navigator.onLine should be true");

    run_json(port, &["emulate", "--offline", "on"]);
    // Reload so navigator.onLine reflects the tab-offline state.
    navigate(port, "data:text/html,<h1>offline probe</h1>");
    let offline = eval(port, "navigator.onLine");
    assert_eq!(
        offline, false,
        "with --offline on, navigator.onLine must be false: {offline}"
    );

    run_json(port, &["emulate", "--offline", "off"]);
    navigate(port, "data:text/html,<h1>offline probe</h1>");
    let restored = eval(port, "navigator.onLine");
    assert_eq!(
        restored, true,
        "after --offline off, navigator.onLine must be true again: {restored}"
    );

    stop_daemon(port);
}

/// `e2e_emulate_one_shot_lifetime_warning`:
///
/// `emulate --no-daemon …` envelope carries the connection-lifetime warning;
/// the daemon-path envelope does not.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn e2e_emulate_one_shot_lifetime_warning() {
    if !live_tests_enabled() {
        eprintln!("e2e_emulate_one_shot_lifetime_warning: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("Firefox not available — skipping");
        return;
    };
    let port = ff.port();

    // One-shot path (--no-daemon): the envelope MUST carry the lifetime warning.
    let one_shot = Command::new(ff_rdp_bin())
        .args(base_args(port))
        .args(["emulate", "--color-scheme", "dark"])
        .output()
        .expect("ff-rdp emulate --no-daemon");
    assert!(
        one_shot.status.success(),
        "emulate --no-daemon must succeed: {}",
        String::from_utf8_lossy(&one_shot.stderr)
    );
    let one_shot_json: Value =
        serde_json::from_slice(&one_shot.stdout).expect("one-shot emulate output is JSON");
    let warning = one_shot_json["results"]["lifetime_warning"]
        .as_str()
        .unwrap_or("");
    assert!(
        warning.contains("one-shot connection only"),
        "one-shot envelope must carry the lifetime warning: {one_shot_json}"
    );

    // Daemon path: start the daemon, then the envelope MUST NOT carry it.
    if ff.with_daemon().is_none() {
        eprintln!("daemon did not start — skipping daemon-path assertion");
        return;
    }
    let daemon_json = run_json(port, &["emulate", "--color-scheme", "dark"]);
    assert!(
        daemon_json["results"].get("lifetime_warning").is_none(),
        "daemon-path envelope must NOT carry the lifetime warning: {daemon_json}"
    );

    stop_daemon(port);
}
