//! Live tests for iteration 300: `launch <URL>` returns only once the tab the
//! next command uses shows the URL's document, loaded — with and without
//! `--temporary-addon`. The very first command after `launch` reads it, with
//! no retry.
//!
//! Before the fix, an `eval` right after a plain `launch --headless <URL>`
//! read the initial `about:blank` (`readyState` `uninitialized`) in 22 of 30
//! runs (Firefox 160, local http page), while `listTabs` already showed the URL.
//!
//! The page is served by a local [`FixtureServer`] over `http:` — no network
//! gate.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live live_300 -- --include-ignored

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

use crate::common::{
    FixtureRoute, FixtureServer, LiveFirefox, base_args, ff_rdp_bin, live_tests_enabled,
    output_note,
};

/// `[href, readyState, <p> count, add-on mark]` of the selected tab.
const PROBE: &str = "JSON.stringify([location.href, document.readyState, \
     document.querySelectorAll('p').length, \
     document.documentElement.getAttribute('data-ff-rdp-temporary-addon')])";

fn serve_page() -> FixtureServer {
    let routes = HashMap::from([(
        "/".to_owned(),
        FixtureRoute::html("<!doctype html><title>t300</title><p>one</p><p>two</p>"),
    )]);
    FixtureServer::start(routes).expect("bind the local fixture HTTP server")
}

/// Launch with `extra` and the page's URL, then read the page once.
fn launch_then_read(extra: &[&str]) -> (String, Value, Value) {
    let server = serve_page();
    let url = format!("{}/", server.base_url());
    let mut args = extra.to_vec();
    args.push(&url);
    let (ff, launch) = LiveFirefox::headless_on_random_port_with_args(&args);
    eprintln!("launch: {launch}");
    assert_eq!(launch["results"]["requested_url"], url.as_str(), "{launch}");
    assert!(launch["results"]["warnings"].is_null(), "{launch}");

    let out = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["eval", PROBE, "--jq", ".results"])
        .output()
        .expect("run ff-rdp eval");
    assert!(
        out.status.success(),
        "first command after launch failed: {}",
        output_note(&out)
    );
    let raw: Value = serde_json::from_slice(&out.stdout).expect("eval JSON");
    let state: Value = serde_json::from_str(raw.as_str().expect("probe string")).expect("probe");
    eprintln!("first read: {state}");
    (url, state, launch)
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_300_plain_launch_url_is_loaded_for_the_next_command() {
    if !live_tests_enabled() {
        eprintln!(
            "live_300_plain_launch_url_is_loaded_for_the_next_command: set FF_RDP_LIVE_TESTS=1"
        );
        return;
    }
    let (url, state, _) = launch_then_read(&[]);
    assert_eq!(state[0], url.as_str(), "{state}");
    assert_eq!(state[1], "complete", "{state}");
    assert_eq!(state[2], 2, "{state}");
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_300_temporary_addon_launch_url_is_loaded_for_the_next_command() {
    if !live_tests_enabled() {
        eprintln!(
            "live_300_temporary_addon_launch_url_is_loaded_for_the_next_command: set FF_RDP_LIVE_TESTS=1"
        );
        return;
    }
    let addon = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/temporary_addon_mv3");
    let addon = addon.to_str().expect("UTF-8 fixture path");
    let (url, state, launch) = launch_then_read(&["--temporary-addon", addon]);
    assert_eq!(
        launch["results"]["temporary_addons"][0]["id"], "temporary-addon-fixture@ff-rdp.test",
        "{launch}"
    );
    assert_eq!(state[0], url.as_str(), "{state}");
    assert_eq!(state[1], "complete", "{state}");
    assert_eq!(state[2], 2, "{state}");
    assert_eq!(state[3], "loaded", "content script mark missing: {state}");
}
