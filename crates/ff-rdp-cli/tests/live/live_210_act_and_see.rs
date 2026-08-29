//! Live tests for iteration 210 — act-and-see.
//!
//! The axi.md browser benchmark (`kb/research/axi-benchmark-comparison.md`, 84
//! runs) measured ff-rdp needing 8 turns where a tool that returns the page
//! after every action needs 4, on every click-through task. Two causes: no
//! state-changing command returned the page it produced, and refs only came out
//! of `dom <selector>` — so an agent had to already know a selector before it
//! could get a clickable handle.
//!
//! Themes covered here:
//! - A: `--with-page` embeds the resulting page under `results.page`, collected
//!   AFTER the action, so a click that navigates reports the destination.
//! - B: `a11y summary` and `snapshot` register `--ref` handles like `dom` does.
//! - C: `type --submit` presses Enter and falls back to `form.requestSubmit()`.
//! - D: a second `launch` on a port ff-rdp already owns is a no-op, exit 0.
//!
//! daemon-parity: every test uses [`daemon_args`] (no `--no-daemon`). The
//! daemon owns the ref store, so `refs_registered` is only ever true on this
//! route — testing the direct route would prove the opposite of the point.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live live_210_act_and_see -- --nocapture

use std::collections::HashMap;
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

/// A two-page fixture: `/` links to `/babbage`, which has its own `<h1>`.
///
/// Deliberately mirrors the benchmark's `wikipedia_link_follow` shape — land
/// on a page, find a link, follow it — because that is the trajectory this
/// iteration is trying to shorten.
fn link_fixture() -> HashMap<String, FixtureRoute> {
    let mut routes = HashMap::new();
    routes.insert(
        "/".to_owned(),
        FixtureRoute::html(
            "<!doctype html><title>t210 origin</title><body>\
             <h1>Ada Lovelace</h1>\
             <a href=\"/babbage\">Charles Babbage</a>\
             <button>Ignore me</button></body>",
        ),
    );
    routes.insert(
        "/babbage".to_owned(),
        FixtureRoute::html(
            "<!doctype html><title>t210 destination</title><body>\
             <h1>Charles Babbage</h1></body>",
        ),
    );
    routes
}

/// The `ref` of the first interactive entry whose `name` matches.
fn ref_named(page: &Value, name: &str) -> String {
    page["interactive"]
        .as_array()
        .unwrap_or_else(|| panic!("page.interactive must be an array: {page}"))
        .iter()
        .find(|e| e["name"] == name)
        .and_then(|e| e["ref"].as_str())
        .unwrap_or_else(|| panic!("no ref for interactive entry named {name:?} in {page}"))
        .to_owned()
}

// ---------------------------------------------------------------------------
// Theme A — `--with-page`
// ---------------------------------------------------------------------------

/// AC `live_navigate_with_page_returns_headings_and_refs`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_navigate_with_page_returns_headings_and_refs() {
    if !live_tests_enabled() {
        eprintln!("live_navigate_with_page_returns_headings_and_refs: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_navigate_with_page_returns_headings_and_refs");
    let port = ff.port();
    let Some(server) = FixtureServer::start(link_fixture()) else {
        eprintln!("live_navigate_with_page_returns_headings_and_refs: no fixture HTTP — skipping");
        stop_daemon(port);
        return;
    };

    let nav = run_json(port, &["navigate", &server.base_url(), "--with-page"]);
    let page = &nav["results"]["page"];
    assert_eq!(
        page["headings"][0]["text"], "Ada Lovelace",
        "page.headings[0] must be the fixture's <h1>: {nav}"
    );
    assert_eq!(
        nav["meta"]["page_source"], "js-fallback",
        "meta.page_source must name how the view was produced: {nav}"
    );
    assert_eq!(
        nav["meta"]["page_refs_registered"], true,
        "the daemon route must register the page's refs: {nav}"
    );

    let interactive = page["interactive"]
        .as_array()
        .unwrap_or_else(|| panic!("page.interactive must be an array: {nav}"));
    assert!(
        !interactive.is_empty(),
        "the fixture has a link and a button: {nav}"
    );
    for entry in interactive {
        let r = entry["ref"]
            .as_str()
            .unwrap_or_else(|| panic!("interactive entry without a ref: {entry} in {nav}"));
        assert!(
            r.starts_with('e') && r[1..].chars().all(|c| c.is_ascii_digit()) && r.len() > 1,
            "ref {r:?} must match ^e\\d+$: {nav}"
        );
    }

    stop_daemon(port);
}

/// AC `live_click_ref_from_with_page_lands_on_target`: the refs `navigate
/// --with-page` hands back are accepted by `click --ref` — the two-command
/// trajectory the benchmark measured at eight.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_click_ref_from_with_page_lands_on_target() {
    if !live_tests_enabled() {
        eprintln!("live_click_ref_from_with_page_lands_on_target: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_click_ref_from_with_page_lands_on_target");
    let port = ff.port();
    let Some(server) = FixtureServer::start(link_fixture()) else {
        eprintln!("live_click_ref_from_with_page_lands_on_target: no fixture HTTP — skipping");
        stop_daemon(port);
        return;
    };

    let nav = run_json(port, &["navigate", &server.base_url(), "--with-page"]);
    let ref_id = ref_named(&nav["results"]["page"], "Charles Babbage");

    let click = run_json(port, &["click", "--ref", &ref_id]);
    assert_eq!(
        click["results"]["clicked"], true,
        "click --ref {ref_id} must click: {click}"
    );
    assert_eq!(
        click["results"]["text"], "Charles Babbage",
        "the clicked element's text must be the ref's name: {click}"
    );

    stop_daemon(port);
}

/// AC `live_click_with_page_reflects_post_click_document`: after clicking a
/// link, the embedded page is the DESTINATION's, not the origin's.
///
/// This is the one that makes `--with-page` worth having: the console actor
/// cached before the click is bound to the old docshell, so a naive
/// implementation returns the page the agent already had.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_click_with_page_reflects_post_click_document() {
    if !live_tests_enabled() {
        eprintln!("live_click_with_page_reflects_post_click_document: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_click_with_page_reflects_post_click_document");
    let port = ff.port();
    let Some(server) = FixtureServer::start(link_fixture()) else {
        eprintln!("live_click_with_page_reflects_post_click_document: no fixture HTTP — skipping");
        stop_daemon(port);
        return;
    };

    let nav = run_json(port, &["navigate", &server.base_url(), "--with-page"]);
    let ref_id = ref_named(&nav["results"]["page"], "Charles Babbage");

    let click = run_json(port, &["click", "--ref", &ref_id, "--with-page"]);
    assert_eq!(
        click["results"]["page"]["headings"][0]["text"], "Charles Babbage",
        "click --with-page must report the destination page's heading, not the origin's: {click}"
    );

    stop_daemon(port);
}

// ---------------------------------------------------------------------------
// Theme B — refs from the read commands
// ---------------------------------------------------------------------------

/// AC `live_a11y_summary_registers_refs`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_a11y_summary_registers_refs() {
    if !live_tests_enabled() {
        eprintln!("live_a11y_summary_registers_refs: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_a11y_summary_registers_refs");
    let port = ff.port();
    let Some(server) = FixtureServer::start(link_fixture()) else {
        eprintln!("live_a11y_summary_registers_refs: no fixture HTTP — skipping");
        stop_daemon(port);
        return;
    };

    run_json(port, &["navigate", &server.base_url()]);
    let summary = run_json(port, &["a11y", "summary"]);
    assert_eq!(
        summary["meta"]["refs_registered"], true,
        "a11y summary must register refs on the daemon route: {summary}"
    );

    let first_ref = summary["results"]["interactive"][0]["ref"]
        .as_str()
        .unwrap_or_else(|| panic!("a11y summary's first interactive entry has no ref: {summary}"))
        .to_owned();
    let click = run_json(port, &["click", "--ref", &first_ref]);
    assert_eq!(
        click["results"]["clicked"], true,
        "a ref from a11y summary must be clickable: {click}"
    );

    stop_daemon(port);
}

/// AC `live_snapshot_interactive_nodes_carry_refs`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_snapshot_interactive_nodes_carry_refs() {
    if !live_tests_enabled() {
        eprintln!("live_snapshot_interactive_nodes_carry_refs: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_snapshot_interactive_nodes_carry_refs");
    let port = ff.port();
    let Some(server) = FixtureServer::start(link_fixture()) else {
        eprintln!("live_snapshot_interactive_nodes_carry_refs: no fixture HTTP — skipping");
        stop_daemon(port);
        return;
    };

    run_json(port, &["navigate", &server.base_url()]);
    let snap = run_json(port, &["snapshot"]);
    assert_eq!(
        snap["meta"]["refs_registered"], true,
        "snapshot must register refs on the daemon route: {snap}"
    );

    let mut interactive = Vec::new();
    collect_interactive(&snap["results"], &mut interactive);
    assert!(
        !interactive.is_empty(),
        "the fixture's <a> and <button> must appear as interactive nodes: {snap}"
    );
    for node in &interactive {
        assert!(
            node["ref"].as_str().is_some_and(|r| r.starts_with('e')),
            "interactive snapshot node without a ref: {node} in {snap}"
        );
    }

    stop_daemon(port);
}

/// Depth-first collection of `interactive: true` nodes from a snapshot tree.
fn collect_interactive(node: &Value, out: &mut Vec<Value>) {
    match node {
        Value::Object(map) => {
            if map.get("interactive") == Some(&Value::Bool(true)) {
                out.push(node.clone());
            }
            if let Some(children) = map.get("children") {
                collect_interactive(children, out);
            }
        }
        Value::Array(arr) => {
            for child in arr {
                collect_interactive(child, out);
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Theme C — `type --submit`
// ---------------------------------------------------------------------------

/// AC `live_type_submit_navigates_search_form`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_type_submit_navigates_search_form() {
    if !live_tests_enabled() {
        eprintln!("live_type_submit_navigates_search_form: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_type_submit_navigates_search_form");
    let port = ff.port();

    let mut routes = HashMap::new();
    routes.insert(
        "/".to_owned(),
        FixtureRoute::html(
            "<!doctype html><title>t210 form</title><body>\
             <form action=\"/results\" method=\"get\">\
             <input name=\"q\" aria-label=\"Search\">\
             </form></body>",
        ),
    );
    routes.insert(
        "/results".to_owned(),
        FixtureRoute::html("<!doctype html><title>t210 results</title><body><h1>Results</h1></body>"),
    );
    let Some(server) = FixtureServer::start(routes) else {
        eprintln!("live_type_submit_navigates_search_form: no fixture HTTP — skipping");
        stop_daemon(port);
        return;
    };

    run_json(port, &["navigate", &server.base_url()]);
    let typed = run_json(port, &["type", "input[name=q]", "turing", "--submit"]);
    assert_eq!(
        typed["results"]["submitted"], true,
        "type --submit must report submitted: {typed}"
    );

    let url = run_json(port, &["eval", "location.href", "--stringify"]);
    let href = format!("{}", url["results"]);
    assert!(
        href.contains("q=turing"),
        "the resulting URL must carry the query: {href} (from {typed})"
    );

    stop_daemon(port);
}

// ---------------------------------------------------------------------------
// Theme D — idempotent `launch`
// ---------------------------------------------------------------------------

/// AC `live_launch_twice_is_a_noop`: a second `launch` on a port ff-rdp
/// already owns exits 0 and reports the SAME pid, rather than failing with
/// "port already in use" — the failure that hit 3 of 42 benchmark runs.
///
/// The foreign-owner half of that AC needs no Firefox and is covered by
/// `unit_210_foreign_port_owner_is_not_a_running_instance` in `launch.rs`,
/// where the ownership probes can be stubbed instead of requiring a real
/// unrelated listener on a real port.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_launch_twice_is_a_noop() {
    if !live_tests_enabled() {
        eprintln!("live_launch_twice_is_a_noop: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    // Not `firefox_with_daemon`: this test launches through the CLI itself,
    // which is the behaviour under test.
    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();

    let again = run(port, &["launch", "--headless", "--debug-port", &port.to_string()]);
    assert!(
        again.status.success(),
        "a second launch on an ff-rdp-owned port must exit 0: stdout={} stderr={}",
        String::from_utf8_lossy(&again.stdout),
        String::from_utf8_lossy(&again.stderr)
    );
    let json: Value = serde_json::from_str(String::from_utf8_lossy(&again.stdout).trim())
        .unwrap_or_else(|e| {
            panic!(
                "second launch output not JSON: {e}\n{}",
                String::from_utf8_lossy(&again.stdout)
            )
        });
    assert_eq!(
        json["results"]["already_running"], true,
        "the no-op launch must say so: {json}"
    );
    assert_eq!(
        json["results"]["pid"].as_u64(),
        Some(u64::from(ff.pid())),
        "the reported pid must be the instance already running: {json}"
    );

    stop_daemon(port);
}
