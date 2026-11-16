//! Live test for `ff-rdp navigate moz-extension://<uuid>/page.html`: an
//! installed add-on's own page opens in the tab, `eval` runs in the extension
//! context (`browser.storage.local` included), and the tab navigates back to
//! an `http:` page with `eval`/`dom` still working.
//!
//! The add-on is the MV3 fixture extension (`page.html` is not listed in
//! `web_accessible_resources`). The `http:` page comes from a local
//! [`FixtureServer`] — no network gate.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live live_moz_extension -- --include-ignored

use std::collections::HashMap;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

use crate::common::{
    FixtureRoute, FixtureServer, LiveFirefox, base_args, ff_rdp_bin, live_tests_enabled,
    output_note,
};

const FIXTURE_ID: &str = "temporary-addon-fixture@ff-rdp.test";

fn ff_rdp(port: u16, args: &[&str]) -> Output {
    Command::new(ff_rdp_bin())
        .args(base_args(port))
        .args(args)
        .output()
        .expect("run ff-rdp")
}

/// Run a command that must succeed and return its stdout JSON.
fn ok_json(port: u16, args: &[&str]) -> Value {
    let out = ff_rdp(port, args);
    assert!(out.status.success(), "{args:?}: {}", output_note(&out));
    serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|_| panic!("{args:?}: {}", output_note(&out)))
}

/// `eval --stringify <script>`, parsed back into JSON.
fn eval_json(port: u16, script: &str) -> Value {
    let reply = ok_json(port, &["eval", "--stringify", script]);
    match &reply["results"] {
        Value::String(text) => serde_json::from_str(text)
            .unwrap_or_else(|_| panic!("eval {script}: not JSON: {reply}")),
        other => other.clone(),
    }
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_moz_extension_page_navigate_eval_and_back() {
    if !live_tests_enabled() {
        eprintln!("live_moz_extension_page_navigate_eval_and_back: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let server = FixtureServer::start(HashMap::from([(
        "/".to_owned(),
        FixtureRoute::html("<!doctype html><title>web</title><h1 id=\"web\">web page</h1>"),
    )]))
    .expect("bind the local fixture HTTP server");
    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();
    let ext = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/temporary_addon_mv3");
    let installed = ok_json(
        port,
        &["addon", "install", ext.to_str().expect("UTF-8 path")],
    );
    assert_eq!(installed["results"]["id"], FIXTURE_ID, "{installed}");
    let listed = ok_json(port, &["addon", "list"]);
    let uuid = listed["results"]
        .as_array()
        .and_then(|all| all.iter().find(|a| a["id"] == FIXTURE_ID))
        .and_then(|a| a["uuid"].as_str())
        .unwrap_or_else(|| panic!("fixture add-on has no uuid: {listed}"))
        .to_owned();

    let web = format!("{}/", server.base_url());
    ok_json(port, &["navigate", &web]);

    let page = format!("moz-extension://{uuid}/page.html");
    let nav = ok_json(port, &["navigate", &page]);
    eprintln!("navigate {page}: {nav}");
    assert_eq!(nav["results"]["committed_url"], page.as_str(), "{nav}");

    let context = eval_json(
        port,
        "[location.protocol, typeof browser.runtime.id, browser.runtime.id, document.title]",
    );
    assert_eq!(
        context,
        serde_json::json!([
            "moz-extension:",
            "string",
            FIXTURE_ID,
            "ff-rdp extension page"
        ]),
        "eval did not run in the extension page"
    );
    let stored = eval_json(
        port,
        "await browser.storage.local.set({ ffRdp: 'stored' }); await browser.storage.local.get()",
    );
    assert_eq!(stored["ffRdp"], "stored", "{stored}");
    let heading = ok_json(port, &["dom", "#extension-page", "--text"]);
    assert_eq!(heading["results"][0], "extension page", "{heading}");

    // Back to the web: the tab leaves the extension process again and every
    // command follows it.
    let back = ok_json(port, &["navigate", &web]);
    assert_eq!(back["results"]["committed_url"], web.as_str(), "{back}");
    let after = eval_json(
        port,
        "[location.href, document.documentElement.getAttribute('data-ff-rdp-temporary-addon')]",
    );
    assert_eq!(
        after,
        serde_json::json!([web, "loaded"]),
        "eval after leaving the extension page"
    );
    let web_heading = ok_json(port, &["dom", "#web", "--text"]);
    assert_eq!(web_heading["results"][0], "web page", "{web_heading}");
}
