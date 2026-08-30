//! Live tests for iteration 212 — ambient context.
//!
//! The unit and e2e suites cover everything the home view can do without a
//! browser. What only a live Firefox can prove is the claim the view makes
//! when there *is* one: that the `tabs` block names the page that is actually
//! loaded, and that the `ref` handles in the `page` block are real — a `ref`
//! `click` cannot resolve is worse than no `ref` at all, because the agent
//! spends a turn discovering it.
//!
//! daemon-parity: these run on the daemon route (no `--no-daemon`) because the
//! daemon owns the ref store, and the whole point of the `page` block is the
//! handles it hands out.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live live_212_ambient_context -- --nocapture

use std::collections::HashMap;
use std::fmt::Write as _;
use std::process::{Command, Output};

use serde_json::Value;

use crate::common::{FixtureRoute, FixtureServer, LiveFirefox, ff_rdp_bin, live_tests_enabled};

fn daemon_args(port: u16) -> Vec<String> {
    vec![
        "--host".to_owned(),
        "127.0.0.1".to_owned(),
        "--port".to_owned(),
        port.to_string(),
        "--timeout".to_owned(),
        "20000".to_owned(),
    ]
}

fn stop_daemon(port: u16) {
    let _ = Command::new(ff_rdp_bin())
        .args(["--host", "127.0.0.1", "--port", &port.to_string()])
        .args(["daemon", "stop"])
        .output();
}

fn firefox_with_daemon(test: &str) -> LiveFirefox {
    let ff = LiveFirefox::headless_on_random_port();
    assert!(
        ff.with_daemon().is_some(),
        "{test}: the proxy daemon did not start for Firefox on port {}",
        ff.port()
    );
    ff
}

fn run(port: u16, args: &[&str]) -> Output {
    Command::new(ff_rdp_bin())
        .args(daemon_args(port))
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("spawn ff-rdp {args:?}: {e}"))
}

fn run_json(port: u16, args: &[&str]) -> Value {
    let out = run(port, args);
    assert!(
        out.status.success(),
        "command {args:?} failed: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("output for {args:?} not JSON: {e}\n{stdout}"))
}

/// A page with one unmistakable heading and one link whose click is
/// observable: following it lands on `/clicked`, so a `ref` that resolves can
/// be told apart from one that merely did not error.
fn fixture() -> HashMap<String, FixtureRoute> {
    let mut routes = HashMap::new();
    routes.insert(
        "/".to_owned(),
        FixtureRoute::html(
            "<!doctype html><title>t212 home</title><body>\
             <h1>Ambient context</h1>\
             <a id=\"go\" href=\"/clicked\">Follow me</a>\
             </body>",
        ),
    );
    routes.insert(
        "/clicked".to_owned(),
        FixtureRoute::html(
            "<!doctype html><title>t212 clicked</title><body><h1>Arrived</h1></body>",
        ),
    );
    routes
}

/// AC `live_home_with_page_lists_tabs_and_refs`: after a navigate, the bare
/// home view's JSON names the loaded URL under `results.tabs[0].url`, and the
/// first `results.page.interactive` entry's `ref` is accepted by `click --ref`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_home_with_page_lists_tabs_and_refs() {
    if !live_tests_enabled() {
        eprintln!("live_home_with_page_lists_tabs_and_refs: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_home_with_page_lists_tabs_and_refs");
    let port = ff.port();
    let Some(server) = FixtureServer::start(fixture()) else {
        eprintln!("live_home_with_page_lists_tabs_and_refs: no fixture HTTP — skipping");
        stop_daemon(port);
        return;
    };
    let url = server.base_url();

    run_json(port, &["navigate", &url]);

    // `--format json` because the home view renders text by default; the hook
    // and any script consume this shape.
    let home = run_json(port, &["--format", "json"]);
    let results = &home["results"];

    assert_eq!(
        results["browser"]["reachable"],
        Value::Bool(true),
        "a live Firefox must read as reachable: {home}"
    );
    let tabs = results["tabs"].as_array().expect("tabs array");
    assert!(!tabs.is_empty(), "the loaded tab must be listed: {home}");
    let listed = tabs[0]["url"]
        .as_str()
        .unwrap_or_else(|| panic!("tab url must be a string: {home}"));
    assert!(
        listed.starts_with(&url),
        "tabs[0].url must name the page that is loaded ({url}), got {listed}: {home}"
    );
    assert_eq!(
        tabs[0]["index"],
        Value::from(1),
        "tab indices are 1-based, matching `--tab N`: {home}"
    );

    let page = &results["page"];
    assert!(
        !page.is_null(),
        "a loaded page must produce a page block: {home}"
    );
    let headings = page["headings"].as_array().expect("headings array");
    assert!(
        headings
            .iter()
            .any(|h| h["text"].as_str() == Some("Ambient context")),
        "the page block must describe the loaded document: {home}"
    );

    let refs_registered = page["refs_registered"].as_bool().unwrap_or(false);
    assert!(
        refs_registered,
        "on the daemon route the page block must carry live refs: {home}"
    );
    let first_ref = page["interactive"]
        .as_array()
        .and_then(|entries| entries.first())
        .and_then(|entry| entry["ref"].as_str())
        .unwrap_or_else(|| panic!("the first interactive entry must carry a ref: {home}"));

    // The ref is only useful if `click` accepts it — the AC's actual claim.
    let clicked = run_json(port, &["click", "--ref", first_ref]);
    assert!(
        clicked["results"]["clicked"] != Value::Bool(false),
        "click --ref {first_ref} must act on the element the home view named: {clicked}"
    );

    let after = run_json(port, &["--format", "json"]);
    let after_url = after["results"]["tabs"][0]["url"]
        .as_str()
        .unwrap_or_default();
    assert!(
        after_url.ends_with("/clicked"),
        "the click must have followed the link the ref pointed at, got {after_url}: {after}"
    );

    // …and the hints an agent reads name that same ref, verbatim.
    let hints = results["hints"].as_array().expect("hints array");
    assert!(
        hints
            .iter()
            .filter_map(Value::as_str)
            .any(|h| h.contains(&format!("--ref {first_ref}"))),
        "the hints must offer the ref the page block minted: {hints:?}"
    );

    stop_daemon(port);
}

/// A browser with nothing loaded must still exit 0 and say so — the state
/// that sends an agent to `navigate` rather than to `launch`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_home_with_blank_tab_asks_for_a_navigate() {
    if !live_tests_enabled() {
        eprintln!("live_home_with_blank_tab_asks_for_a_navigate: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_home_with_blank_tab_asks_for_a_navigate");
    let port = ff.port();

    run_json(port, &["navigate", "about:blank"]);

    let out = run(port, &["--format", "json"]);
    assert!(
        out.status.success(),
        "the home view exits 0 whatever the browser state: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let home: Value = serde_json::from_slice(&out.stdout).expect("JSON");
    let results = &home["results"];
    assert_eq!(results["browser"]["reachable"], Value::Bool(true), "{home}");
    assert!(
        results["page"].is_null(),
        "about:blank is not a page worth describing: {home}"
    );
    let hints: Vec<&str> = results["hints"]
        .as_array()
        .expect("hints array")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(
        hints.iter().any(|h| h.starts_with("ff-rdp navigate")),
        "a blank tab must be told to navigate: {hints:?}"
    );
    assert!(
        !hints.iter().any(|h| h.contains("--ref")),
        "no refs exist on a blank tab, so none may be offered: {hints:?}"
    );

    stop_daemon(port);
}

/// The `--hook` form is what a session hook runs on every session, so its
/// output has to stay small: landmarks dropped, interactive capped at 15.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_home_hook_form_is_trimmed() {
    if !live_tests_enabled() {
        eprintln!("live_home_hook_form_is_trimmed: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_home_hook_form_is_trimmed");
    let port = ff.port();

    // 40 links: more than the hook's 15-entry budget, fewer than the default 50.
    let links = (0..40).fold(String::new(), |mut acc, i| {
        let _ = write!(acc, "<a href=\"/#{i}\">link {i}</a>");
        acc
    });
    let mut routes = HashMap::new();
    routes.insert(
        "/".to_owned(),
        FixtureRoute::html(format!(
            "<!doctype html><title>t212 many</title><body><nav><h1>Many</h1>{links}</nav></body>"
        )),
    );
    let Some(server) = FixtureServer::start(routes) else {
        eprintln!("live_home_hook_form_is_trimmed: no fixture HTTP — skipping");
        stop_daemon(port);
        return;
    };
    run_json(port, &["navigate", &server.base_url()]);

    let full = run_json(port, &["home", "--format", "json"]);
    let hook = run_json(port, &["home", "--hook", "--format", "json"]);

    let full_interactive = full["results"]["page"]["interactive"]
        .as_array()
        .map_or(0, Vec::len);
    let hook_interactive = hook["results"]["page"]["interactive"]
        .as_array()
        .map_or(0, Vec::len);
    assert!(
        full_interactive > hook_interactive,
        "the hook form must cut the interactive list down ({full_interactive} vs {hook_interactive})"
    );
    assert!(
        hook_interactive <= 15,
        "the hook form keeps at most 15 interactive entries, got {hook_interactive}: {hook}"
    );
    assert!(
        hook["results"]["page"].get("landmarks").is_none(),
        "the hook form drops landmarks: {hook}"
    );
    assert!(
        hook["results"]["page"]["headings"]
            .as_array()
            .is_some_and(|h| !h.is_empty()),
        "…but keeps the headings that name the page: {hook}"
    );

    stop_daemon(port);
}
