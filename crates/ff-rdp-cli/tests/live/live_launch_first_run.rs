//! `ff-rdp launch` first-run hygiene against a real Firefox:
//!
//! - the Terms-of-Use / pre-onboarding modal and the data-reporting notice
//!   are off — asserted through Firefox's own decision
//!   (`TelemetryReportingPolicy.willShowTOUModal()` in the parent process),
//!   not just the pref values;
//! - `--auto-consent` returns with exactly one tab: Consent-O-Matic's
//!   first-run `options.html` tab is closed before `launch` returns and does
//!   not come back;
//! - a Firefox launched with a user `--profile` can be stopped with
//!   `ff-rdp stop`, which keeps the user's directory.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live live_launch_first_run -- --include-ignored

use std::process::Command;
use std::time::Duration;

use ff_rdp_core::types::Grip;
use ff_rdp_core::{RdpConnection, RootActor, TabActor, WebConsoleActor};
use serde_json::Value;

use crate::common::{
    LiveFirefox, base_args, ff_rdp_bin, ff_rdp_launch_command, guard_launched_firefox,
    live_tests_enabled, output_note,
};

/// Evaluate `js` (which must produce a string) in Firefox's parent process.
fn parent_eval(port: u16, js: &str) -> String {
    let mut connection =
        RdpConnection::connect("127.0.0.1", port, Duration::from_secs(10)).expect("connect");
    let transport = connection.transport_mut();
    let descriptor = RootActor::get_process(transport, 0).expect("getProcess");
    let target = TabActor::get_process_target(transport, &descriptor).expect("process target");
    let result =
        WebConsoleActor::evaluate_js_async(transport, &target.console_actor, js).expect("eval");
    assert!(result.exception.is_none(), "parent eval threw: {result:?}");
    match result.result {
        Grip::Value(Value::String(s)) => s,
        other => panic!("parent eval returned {other:?}"),
    }
}

/// Raw `listTabs` — unlike `ff-rdp tabs`, nothing is filtered.
fn raw_tab_urls(port: u16) -> Vec<String> {
    let mut connection =
        RdpConnection::connect("127.0.0.1", port, Duration::from_secs(10)).expect("connect");
    RootActor::list_tabs(connection.transport_mut())
        .expect("listTabs")
        .iter()
        .map(|tab| tab.url.clone())
        .collect()
}

// The module URL is split so the iter-242 third-party-host scanner, which
// reads every `scheme://host` literal, does not take Gecko's `gre` for a host.
const TOU_STATE_JS: &str = r#"(() => {
  const { TelemetryReportingPolicy: p } = ChromeUtils.importESModule(
    "resource:" + "//gre/modules/TelemetryReportingPolicy.sys.mjs");
  return JSON.stringify({
    willShowTOUModal: p.willShowTOUModal(),
    shouldNotifyDataReportingPolicy: p.testShouldNotify(),
    canUpload: p.canUpload(),
    preonboarding: Services.prefs.getBoolPref("browser.preonboarding.enabled"),
    touBypass: Services.prefs.getBoolPref("termsofuse.bypassNotification"),
  });
})()"#;

fn assert_no_first_run_modal(test: &str, port: u16) {
    let state: Value = serde_json::from_str(&parent_eval(port, TOU_STATE_JS)).expect("json");
    assert_eq!(state["willShowTOUModal"], false, "{test}: {state}");
    assert_eq!(
        state["shouldNotifyDataReportingPolicy"], false,
        "{test}: {state}"
    );
    assert_eq!(
        state["canUpload"], false,
        "{test}: telemetry must stay off: {state}"
    );
    assert_eq!(state["preonboarding"], false, "{test}: {state}");
    assert_eq!(state["touBypass"], true, "{test}: {state}");
}

#[test]
#[ignore = "requires Firefox + FF_RDP_LIVE_TESTS=1"]
fn live_launch_first_run_terms_of_use_modal_is_off() {
    const TEST: &str = "live_launch_first_run_terms_of_use_modal_is_off";
    if !live_tests_enabled() {
        eprintln!("{TEST}: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }
    let ff = LiveFirefox::headless_on_random_port();
    assert_no_first_run_modal(TEST, ff.port());
}

#[test]
#[ignore = "requires Firefox + FF_RDP_LIVE_TESTS=1"]
fn live_launch_first_run_auto_consent_leaves_one_tab() {
    const TEST: &str = "live_launch_first_run_auto_consent_leaves_one_tab";
    if !live_tests_enabled() {
        eprintln!("{TEST}: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }
    let (ff, launch) = LiveFirefox::headless_on_random_port_with_args(&["--auto-consent"]);
    assert!(
        launch["results"].get("warnings").is_none_or(Value::is_null),
        "{TEST}: the options-tab closer must have closed the tab: {launch}"
    );
    // Straight after `launch` returns, and again after the extension had
    // every chance to open its tab a second time.
    for pause in [Duration::ZERO, Duration::from_secs(3)] {
        std::thread::sleep(pause);
        let urls = raw_tab_urls(ff.port());
        assert_eq!(
            urls.len(),
            1,
            "{TEST}: expected exactly one tab after {pause:?}: {urls:?}"
        );
        assert!(
            !urls[0].starts_with("moz-extension://"),
            "{TEST}: the remaining tab is the extension's: {urls:?}"
        );
    }
}

#[test]
#[ignore = "requires Firefox + FF_RDP_LIVE_TESTS=1"]
fn live_launch_first_run_user_profile_can_be_stopped() {
    const TEST: &str = "live_launch_first_run_user_profile_can_be_stopped";
    if !live_tests_enabled() {
        eprintln!("{TEST}: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }
    let user_profile = tempfile::tempdir().expect("user profile");
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .expect("free port")
        .port();
    let out = ff_rdp_launch_command()
        .args(["launch", "--headless", "--auto-consent"])
        .args(["--profile", user_profile.path().to_str().expect("utf-8")])
        .args(["--debug-port", &port.to_string()])
        .output()
        .expect("run ff-rdp launch");
    let guard = guard_launched_firefox(&out);
    assert!(
        out.status.success(),
        "{TEST}: launch failed — {}",
        output_note(&out)
    );
    let launched: Value = serde_json::from_slice(&out.stdout).expect("launch json");
    assert_no_first_run_modal(TEST, port);
    // The extension is new to this profile, so `launch` waited for and closed
    // its options tab before returning.
    let urls = raw_tab_urls(port);
    assert_eq!(urls.len(), 1, "{TEST}: expected exactly one tab: {urls:?}");

    // `ff_rdp_launch_command` pins this thread's FF_RDP_HOME; stop must use it too.
    let mut stop = ff_rdp_launch_command();
    let out = stop
        .args(base_args(port))
        .arg("stop")
        .output()
        .expect("run ff-rdp stop");
    assert!(
        out.status.success(),
        "{TEST}: stop refused — {}",
        output_note(&out)
    );
    let stopped: Value = serde_json::from_slice(&out.stdout).expect("stop json");
    assert_eq!(stopped["results"]["stopped"], true, "{TEST}: {stopped}");
    assert_eq!(
        stopped["results"]["pid"], launched["results"]["pid"],
        "{TEST}: {stopped}"
    );
    assert_eq!(
        stopped["results"]["profile_removed"], false,
        "{TEST}: {stopped}"
    );
    let reported = stopped["results"]["profile"].as_str().expect("profile");
    assert_eq!(
        std::fs::canonicalize(reported).ok(),
        std::fs::canonicalize(user_profile.path()).ok(),
        "{TEST}: stop must report the user's profile: {stopped}"
    );
    assert!(
        user_profile.path().join("prefs.js").is_file(),
        "{TEST}: the user's profile must survive stop"
    );
    if let Some(guard) = guard {
        guard.disarm();
    }
    // Nothing is listening any more.
    let out = Command::new(ff_rdp_bin())
        .args(base_args(port))
        .arg("tabs")
        .output()
        .expect("run tabs");
    assert!(
        !out.status.success(),
        "{TEST}: Firefox still answers after stop"
    );
}
