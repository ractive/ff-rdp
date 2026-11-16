//! Live tests for `ff-rdp addon install|list|reload|uninstall` against a
//! Firefox that is already running.
//!
//! The add-on is a temporary copy of the MV3 fixture extension, so the test
//! can rewrite its content script's mark between install and reload. Pages are
//! served by a local [`FixtureServer`] over `http:` — no network gate.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live live_298 -- --include-ignored

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::common::{
    FixtureRoute, FixtureServer, IsolatedLiveFirefox, LiveFirefox, ProfilePreference, base_args,
    ff_rdp_bin, live_tests_enabled, output_note,
};

const FIXTURE_ID: &str = "temporary-addon-fixture@ff-rdp.test";
const MARK: &str = "document.documentElement.getAttribute('data-ff-rdp-temporary-addon')";

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

/// A writable copy of the MV3 fixture extension whose content script sets
/// `mark`.
fn extension_copy(dir: &Path, mark: &str) -> PathBuf {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/temporary_addon_mv3");
    let ext = dir.join("ext");
    std::fs::create_dir_all(&ext).expect("create extension copy");
    std::fs::copy(src.join("manifest.json"), ext.join("manifest.json")).expect("copy manifest");
    set_mark(&ext, mark);
    ext
}

fn set_mark(ext: &Path, mark: &str) {
    std::fs::write(
        ext.join("content.js"),
        format!(
            "document.documentElement.setAttribute(\"data-ff-rdp-temporary-addon\", \"{mark}\");\n"
        ),
    )
    .expect("write content.js");
}

/// Evaluate `expr` (which must produce a JSON-serialisable value) in the
/// selected tab until `done` accepts it or 15 s pass.
fn eval_until(port: u16, expr: &str, done: impl Fn(&Value) -> bool) -> Value {
    let probe = format!("JSON.stringify({expr})");
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut last = Value::Null;
    while Instant::now() < deadline {
        let out = ff_rdp(port, &["eval", &probe, "--jq", ".results"]);
        if out.status.success()
            && let Ok(Value::String(inner)) = serde_json::from_slice::<Value>(&out.stdout)
            && let Ok(value) = serde_json::from_str::<Value>(&inner)
        {
            if done(&value) {
                return value;
            }
            last = value;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    panic!("eval {expr} never matched; last value: {last}");
}

fn listed_fixture(port: u16) -> Option<Value> {
    let listed = ok_json(port, &["addon", "list"]);
    listed["results"]
        .as_array()
        .and_then(|all| all.iter().find(|a| a["id"] == FIXTURE_ID).cloned())
}

/// Install, list, reload with a changed content script, uninstall — in one
/// running Firefox. The reload keeps the tab (its document is not reloaded)
/// and the UUID; the next page load carries the new mark.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_298_install_list_reload_uninstall_in_running_firefox() {
    if !live_tests_enabled() {
        eprintln!(
            "live_298_install_list_reload_uninstall_in_running_firefox: set FF_RDP_LIVE_TESTS=1"
        );
        return;
    }
    let server = FixtureServer::start(HashMap::from([
        (
            "/".to_owned(),
            FixtureRoute::html("<!doctype html><title>t298</title><p>first</p>"),
        ),
        (
            "/second".to_owned(),
            FixtureRoute::html("<!doctype html><title>t298b</title><p>second</p>"),
        ),
    ]))
    .expect("bind the local fixture HTTP server");
    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();
    let tmp = tempfile::tempdir().expect("temp dir");
    let ext = extension_copy(tmp.path(), "v1");
    let ext_arg = ext.to_str().expect("UTF-8 path");

    let installed = ok_json(port, &["addon", "install", ext_arg]);
    eprintln!("install: {installed}");
    assert_eq!(installed["results"]["id"], FIXTURE_ID, "{installed}");
    assert!(Path::new(installed["results"]["path"].as_str().unwrap()).is_absolute());

    let listed = listed_fixture(port).expect("installed add-on is listed");
    eprintln!("list: {listed}");
    assert_eq!(listed["temporarily_installed"], true, "{listed}");
    let uuid = listed["uuid"].as_str().expect("uuid").to_owned();

    let first = format!("{}/", server.base_url());
    ok_json(port, &["navigate", &first]);
    eval_until(port, MARK, |v| v == "v1");
    ok_json(
        port,
        &["eval", "window.__ff_rdp_298 = 'old document'; 'set'"],
    );
    let tabs_before = ok_json(port, &["tabs"]);

    set_mark(&ext, "v2");
    let reloaded = ok_json(port, &["addon", "reload", ext_arg]);
    eprintln!("reload: {reloaded}");
    assert_eq!(reloaded["results"]["id"], FIXTURE_ID, "{reloaded}");
    assert_eq!(
        reloaded["results"]["previous_uuid"],
        uuid.as_str(),
        "{reloaded}"
    );
    assert_eq!(reloaded["results"]["uuid"], uuid.as_str(), "{reloaded}");

    // Same tab, same document (the page was not reloaded); Firefox injected
    // the new content script into it.
    let tabs_after = ok_json(port, &["tabs"]);
    assert_eq!(tabs_after["total"], tabs_before["total"], "{tabs_after}");
    assert_eq!(
        tabs_after["results"][0]["url"],
        first.as_str(),
        "{tabs_after}"
    );
    let old = eval_until(
        port,
        &format!("[{MARK}, window.__ff_rdp_298 ?? null]"),
        |v| v[0] == "v2",
    );
    assert_eq!(old[1], "old document", "the open page was reloaded: {old}");

    let second = format!("{}/second", server.base_url());
    ok_json(port, &["navigate", &second]);
    eval_until(port, &format!("[location.href, {MARK}]"), |v| {
        v[0] == second.as_str() && v[1] == "v2"
    });

    let removed = ok_json(port, &["addon", "uninstall", FIXTURE_ID]);
    assert_eq!(removed["results"]["uninstalled"], true, "{removed}");
    assert!(
        listed_fixture(port).is_none(),
        "still listed after uninstall"
    );

    let again = ff_rdp(port, &["addon", "uninstall", FIXTURE_ID]);
    assert!(!again.status.success());
    let err: Value = serde_json::from_slice(&again.stdout).expect("error envelope");
    assert_eq!(err["error_type"], "User", "{err}");
    assert!(
        err["error"]
            .as_str()
            .unwrap_or_default()
            .contains("no add-on with this id"),
        "{err}"
    );
}

/// Install, uninstall, install again; return the UUIDs of both installs.
fn uuids_across_uninstall(port: u16, ext: &str) -> (String, String) {
    let mut uuids = Vec::new();
    for _ in 0..2 {
        ok_json(port, &["addon", "install", ext]);
        let listed = listed_fixture(port).expect("installed add-on is listed");
        uuids.push(listed["uuid"].as_str().expect("uuid").to_owned());
        ok_json(port, &["addon", "uninstall", FIXTURE_ID]);
    }
    (uuids.remove(0), uuids.remove(0))
}

/// A real uninstall drops the UUID unless the profile sets
/// `extensions.webextensions.keepUuidOnUninstall` — the pref `addon reload
/// --help` documents.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_298_uninstall_drops_uuid_unless_keep_uuid_pref() {
    if !live_tests_enabled() {
        eprintln!("live_298_uninstall_drops_uuid_unless_keep_uuid_pref: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let tmp = tempfile::tempdir().expect("temp dir");
    let ext = extension_copy(tmp.path(), "v1");
    let ext_arg = ext.to_str().expect("UTF-8 path");

    let plain = LiveFirefox::headless_on_random_port();
    let (first, second) = uuids_across_uninstall(plain.port(), ext_arg);
    eprintln!("without pref: {first} -> {second}");
    assert_ne!(first, second, "UUID survived an uninstall without the pref");
    drop(plain);

    let keep = (
        "extensions.webextensions.keepUuidOnUninstall".to_owned(),
        ProfilePreference::Bool(true),
    );
    let session = IsolatedLiveFirefox::launch_with_preferences(&ff_rdp_bin(), &[keep])
        .unwrap_or_else(|why| panic!("live_298 keepUuidOnUninstall launch: {why}"));
    let (first, second) = uuids_across_uninstall(session.firefox().port(), ext_arg);
    eprintln!("with keepUuidOnUninstall: {first} -> {second}");
    assert_eq!(first, second, "keepUuidOnUninstall did not keep the UUID");
    session.finish().expect("stop the isolated Firefox");
}
