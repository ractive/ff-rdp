//! Live tests for iteration 297: `launch --temporary-addon <path>` installs an
//! unsigned MV3 extension over RDP before the startup URL opens, so its
//! `document_start` content script already ran on the first page.
//!
//! The page is served by a local [`FixtureServer`] over `http:` — no network
//! gate. A `data:` page cannot stand in: Firefox does not run content scripts
//! in a top-level `data:` document (observed on Firefox 160), and `file:`
//! navigation crashes Firefox 158.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live live_297 -- --include-ignored

use std::collections::HashMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::common::{
    FixtureRoute, FixtureServer, LiveFirefox, assert_no_managed_profiles, base_args,
    current_live_home, ff_rdp_bin, ff_rdp_launch_command, guard_launched_firefox,
    live_tests_enabled,
};

const FIXTURE_ID: &str = "temporary-addon-fixture@ff-rdp.test";

fn fixture_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn serve_page() -> FixtureServer {
    let routes = HashMap::from([(
        "/".to_owned(),
        FixtureRoute::html("<!doctype html><title>t297</title><p>first page</p>"),
    )]);
    FixtureServer::start(routes).expect("bind the local fixture HTTP server")
}

/// Poll the launched Firefox until its tab shows `url` fully loaded, then
/// return `[href, mark, navigation type]` read by one `eval`.
fn first_page_state(port: u16, url: &str) -> Value {
    let probe = "JSON.stringify([location.href, document.readyState, \
         document.documentElement.getAttribute('data-ff-rdp-temporary-addon'), \
         performance.getEntriesByType('navigation')[0]?.type ?? null])";
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut last = String::new();
    while Instant::now() < deadline {
        let out = Command::new(ff_rdp_bin())
            .args(base_args(port))
            .args(["eval", probe, "--jq", ".results"])
            .output()
            .expect("run ff-rdp eval");
        String::from_utf8_lossy(&out.stdout)
            .trim()
            .clone_into(&mut last);
        if out.status.success()
            && let Ok(Value::String(inner)) = serde_json::from_str::<Value>(&last)
            && let Ok(state) = serde_json::from_str::<Value>(&inner)
            && state[0] == url
            && state[1] == "complete"
        {
            return state;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    panic!("tab never finished loading {url}; last eval output: {last}");
}

/// Launch with `addon` and a URL; the first document carries the content
/// script's mark and was a plain navigation, not a reload.
fn assert_addon_marks_first_page(addon: &Path) {
    let server = serve_page();
    let url = format!("{}/", server.base_url());
    let addon_arg = addon.to_str().expect("UTF-8 fixture path");
    let (ff, launch) =
        LiveFirefox::headless_on_random_port_with_args(&["--temporary-addon", addon_arg, &url]);
    eprintln!("launch: {launch}");
    let installed = &launch["results"]["temporary_addons"];
    assert_eq!(installed[0]["id"], FIXTURE_ID, "{launch}");
    assert!(
        Path::new(installed[0]["path"].as_str().unwrap_or_default()).is_absolute(),
        "{launch}"
    );
    assert_eq!(launch["results"]["requested_url"], url.as_str(), "{launch}");
    assert!(launch["results"]["warnings"].is_null(), "{launch}");

    // Launch returned after the navigation committed: the very next page
    // command works on the new document, with no retry. Before the commit
    // wait this failed 5/5 (noSuchActor, tabDestroyed or a 10 s recv timeout).
    let immediate = Command::new(ff_rdp_bin())
        .args(base_args(ff.port()))
        .args(["dom", "html[data-ff-rdp-temporary-addon=loaded]", "--count"])
        .args(["--jq", ".results.count"])
        .output()
        .expect("run ff-rdp dom");
    assert!(
        immediate.status.success(),
        "first command after launch failed: {}",
        crate::common::output_note(&immediate)
    );
    assert_eq!(String::from_utf8_lossy(&immediate.stdout).trim(), "1");

    let state = first_page_state(ff.port(), &url);
    eprintln!("first page: {state}");
    assert_eq!(state[2], "loaded", "content script mark missing: {state}");
    assert_eq!(
        state[3], "navigate",
        "first page must not be a reload: {state}"
    );
}

#[test]
#[ignore = "flaky: see kb/backlog.md"]
fn live_297_directory_addon_marks_first_page() {
    if !live_tests_enabled() {
        eprintln!("live_297_directory_addon_marks_first_page: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    assert_addon_marks_first_page(&fixture_dir("temporary_addon_mv3"));
}

/// The same fixture packed as an `.xpi` at test time (no committed binary).
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_297_xpi_addon_marks_first_page() {
    if !live_tests_enabled() {
        eprintln!("live_297_xpi_addon_marks_first_page: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let tmp = tempfile::tempdir().expect("temp dir");
    let xpi = tmp.path().join("temporary-addon-fixture.xpi");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&xpi).expect("create .xpi"));
    for name in ["manifest.json", "content.js"] {
        let bytes = std::fs::read(fixture_dir("temporary_addon_mv3").join(name)).expect("read");
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .expect("zip entry");
        zip.write_all(&bytes).expect("zip write");
    }
    zip.finish().expect("finish .xpi");
    assert_addon_marks_first_page(&xpi);
}

/// A manifest Firefox rejects fails the launch with Firefox's own message,
/// and the Firefox the launch started is stopped: its port closes and its
/// managed profile is removed (only done after the child was reaped).
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_297_bad_manifest_fails_launch_and_stops_firefox() {
    if !live_tests_enabled() {
        eprintln!("live_297_bad_manifest_fails_launch_and_stops_firefox: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .expect("free port")
        .port();
    let bad = fixture_dir("temporary_addon_bad_manifest");
    let out = ff_rdp_launch_command()
        .args(["launch", "--headless", "--debug-port", &port.to_string()])
        .arg("--temporary-addon")
        .arg(&bad)
        .output()
        .expect("run ff-rdp launch");
    let _guard = guard_launched_firefox(&out);
    let stdout = String::from_utf8_lossy(&out.stdout);
    eprintln!("launch: {stdout}");
    assert!(!out.status.success(), "a rejected install must fail launch");
    let json: Value = serde_json::from_str(stdout.trim()).expect("error envelope JSON");
    let message = json["error"].as_str().unwrap_or_default();
    assert!(
        message.contains("Firefox rejected the temporary install")
            && message.contains("Property \"version\" is required"),
        "{json}"
    );
    assert!(
        std::net::TcpStream::connect(("127.0.0.1", port)).is_err(),
        "the launched Firefox still listens on {port}"
    );
    assert_no_managed_profiles(&current_live_home().join("ff-rdp/profiles"));
}
