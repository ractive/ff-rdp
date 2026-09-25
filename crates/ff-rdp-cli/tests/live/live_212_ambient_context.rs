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

#[path = "../common/home_ref_flow.rs"]
mod home_ref_flow;

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
    home_ref_flow::check(&url, |args| run_json(port, args));

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
/// Its next steps must also drive the first real actions without confusing a
/// daemon ref with a CSS selector (iter 270).
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_home_hook_form_is_trimmed() {
    if !live_tests_enabled() {
        eprintln!("live_home_hook_form_is_trimmed: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_home_hook_form_is_trimmed");
    let port = ff.port();

    // One link plus 40 inputs exceeds the hook's 15-entry budget but stays below
    // the default 50. Both action kinds remain visible after link-first grouping.
    let inputs = (0..40).fold(String::new(), |mut acc, i| {
        let _ = write!(acc, "<input aria-label=\"Search {i}\" type=\"text\">");
        acc
    });
    let mut routes = HashMap::new();
    routes.insert(
        "/".to_owned(),
        FixtureRoute::html(format!(
            "<!doctype html><title>t212 many</title><body><main><h1>Many</h1>\
             <form><a href=\"/clicked\">first action</a>\
             {inputs}</form></main></body>"
        )),
    );
    routes.insert(
        "/clicked".to_owned(),
        FixtureRoute::html(
            "<!doctype html><title>t270 clicked</title><body><h1>Hook action arrived</h1></body>",
        ),
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
    assert_eq!(hook["results"]["page"]["interactive_total"], 41);
    assert_eq!(hook["results"]["page"]["interactive_truncated"], true);
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

    let json_first_ref = hook["results"]["page"]["interactive"]
        .as_array()
        .and_then(|entries| entries.first())
        .and_then(|entry| entry["ref"].as_str())
        .unwrap_or_else(|| panic!("hook page must mint a first-action ref: {hook}"));
    let json_input_ref = hook["results"]["page"]["interactive"]
        .as_array()
        .and_then(|entries| {
            entries.iter().find(|entry| {
                entry["role"].as_str() == Some("input")
                    && entry["name"].as_str() == Some("Search 0")
            })
        })
        .and_then(|entry| entry["ref"].as_str())
        .unwrap_or_else(|| panic!("hook page must mint the input ref: {hook}"));

    let hook_text_out = run(port, &["home", "--hook"]);
    assert!(
        hook_text_out.status.success(),
        "text hook failed: stdout={} stderr={}",
        String::from_utf8_lossy(&hook_text_out.stdout),
        String::from_utf8_lossy(&hook_text_out.stderr)
    );
    let hook_text = String::from_utf8_lossy(&hook_text_out.stdout);
    assert!(
        hook_text.len() < 2_000,
        "the hook remains a compact orientation payload ({} bytes): {hook_text}",
        hook_text.len()
    );
    let action_ref = hook_text
        .lines()
        .find_map(|line| line.strip_prefix("-> ff-rdp click --ref "))
        .and_then(|rest| rest.split_whitespace().next())
        .unwrap_or_else(|| panic!("trimmed hook must print a concrete action ref: {hook_text}"));
    // Resolve the placeholder from the same trimmed text payload an agent sees,
    // not from ordinary home or a ref minted by the earlier JSON invocation.
    let input_ref = hook_text
        .lines()
        .find_map(|line| {
            line.trim()
                .strip_prefix('[')?
                .strip_suffix("] input \"Search 0\"")
        })
        .unwrap_or_else(|| panic!("trimmed text hook must expose the input ref: {hook_text}"));
    assert!(input_ref.starts_with('e'), "{hook_text}");
    assert!(
        action_ref.starts_with('e') && action_ref != "<minted-ref>",
        "the page-bearing hook must print a real minted ref: {hook_text}"
    );
    assert!(
        json_first_ref.starts_with('e') && json_input_ref.starts_with('e'),
        "the JSON hook form must also mint both action refs: {hook}"
    );
    for expected in [
        "navigate <URL> --with-page --query \"<text>\"".to_owned(),
        format!("click --ref {action_ref} --with-page"),
        "type --ref <input-ref> --text \"<text>\" --with-page".to_owned(),
        "click \"<css>\" --with-page  # CSS is positional".to_owned(),
    ] {
        assert!(
            hook_text.contains(&expected),
            "trimmed hook is missing {expected:?}: {hook_text}"
        );
    }

    // Exercise the two ref forms the actual trimmed hook advertises. This is
    // deliberately live: string-only idiom tests cannot prove the hook minted
    // handles that the first action accepts.
    let typed = run_json(
        port,
        &["type", "--ref", input_ref, "--text", "hello", "--with-page"],
    );
    assert_eq!(typed["results"]["typed"], Value::Bool(true), "{typed}");
    let clicked = run_json(port, &["click", "--ref", action_ref, "--with-page"]);
    assert_eq!(
        clicked["results"]["clicked"],
        Value::Bool(true),
        "{clicked}"
    );
    assert_eq!(
        clicked["results"]["page"]["headings"][0]["text"],
        Value::String("Hook action arrived".to_owned()),
        "the advertised first action must reach its destination: {clicked}"
    );

    stop_daemon(port);
}
