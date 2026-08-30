//! Live tests for iteration 211 — find, don't guess.
//!
//! The axi.md benchmark (`kb/research/axi-benchmark-comparison.md`) measured
//! the extraction tasks costing ff-rdp 9.3 turns against 4, and 10.7 against
//! 7, with every trajectory the same loop: `page-text | head -100` (the answer
//! is further down), a guessed `dom` selector, then three to six `eval`
//! scripts until one hit. Three causes, one per theme:
//!
//! - A: no read command could be asked "show me the part of the page
//!   containing X" — `--query` on `page-text`, `snapshot`, `a11y summary`
//!   and `dom`.
//! - B: `page-text` was the only read command with no size cap, which is what
//!   the `| head -100` was working around — and why the answer got cut off.
//! - C: `dom --text` and the ARIA-tree `name` returned `textContent` sliced at
//!   100 characters, so a `<h3><a>Bug: <span>…</span></a></h3>` issue title
//!   came back partial.
//!
//! daemon-parity: the `a11y summary` test uses the daemon route (no
//! `--no-daemon`) because the daemon owns the ref store and the point of that
//! AC is that refs survive filtering. The rest are route-agnostic and use the
//! same route for consistency.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live live_211_find_not_guess -- --nocapture

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

/// 60 `<p>` lines with the needle on line 40 — the AC's fixture shape, and a
/// miniature of the "the answer is past `head -100`" failure.
fn needle_fixture() -> HashMap<String, FixtureRoute> {
    let body: String = (1..=60)
        .map(|n| {
            if n == 40 {
                "<p>the needle is here</p>".to_owned()
            } else {
                format!("<p>filler line {n}</p>")
            }
        })
        .collect();
    let mut routes = HashMap::new();
    routes.insert(
        "/".to_owned(),
        FixtureRoute::html(format!(
            "<!doctype html><title>t211 needle</title><body>{body}</body>"
        )),
    );
    // 20 000 characters of body text, for the default-cap AC. One long
    // paragraph rather than many, so `innerText` adds no newlines of its own
    // and the character count is exactly what the fixture wrote.
    routes.insert(
        "/long".to_owned(),
        FixtureRoute::html(format!(
            "<!doctype html><title>t211 long</title><body><p>{}</p></body>",
            "x".repeat(20_000)
        )),
    );
    routes
}

/// A table whose only `1804` sits three levels down, plus an unrelated
/// heading and second row that pruning must remove.
fn table_fixture() -> HashMap<String, FixtureRoute> {
    let mut routes = HashMap::new();
    routes.insert(
        "/".to_owned(),
        FixtureRoute::html(
            "<!doctype html><title>t211 table</title><body>\
             <h1>World population</h1>\
             <table><tbody>\
             <tr><td>1804</td><td>1 billion</td></tr>\
             <tr><td>1927</td><td>2 billion</td></tr>\
             </tbody></table></body>",
        ),
    );
    routes
}

/// A link list plus the GitHub-issue title shape that produced the benchmark's
/// only outright failure.
fn names_fixture() -> HashMap<String, FixtureRoute> {
    let mut routes = HashMap::new();
    routes.insert(
        "/".to_owned(),
        FixtureRoute::html(
            "<!doctype html><title>t211 names</title><body>\
             <h1>Ada Lovelace</h1>\
             <h3><a href=\"/1\">Bug: <span>title</span></a></h3>\
             <a href=\"/babbage\">Charles Babbage</a>\
             <a href=\"/engine\">Analytical Engine</a>\
             <button>Ignore me</button></body>",
        ),
    );
    routes
}

/// Walk a snapshot tree to its first leaf-most object node, following the
/// single surviving child at each step. Panics with the tree if a level has
/// anything other than exactly one object child.
fn only_child_chain(root: &Value) -> Vec<String> {
    let mut tags = Vec::new();
    let mut node = root;
    while let Some(tag) = node["tag"].as_str() {
        tags.push(tag.to_owned());
        let Some(children) = node["children"].as_array() else {
            break;
        };
        let objects: Vec<&Value> = children.iter().filter(|c| c.is_object()).collect();
        match objects.len() {
            0 => break,
            1 => node = objects[0],
            n => panic!("expected a single surviving child at <{tag}>, got {n}: {root}"),
        }
    }
    tags
}

// ---------------------------------------------------------------------------
// Theme A + B — `page-text`
// ---------------------------------------------------------------------------

/// AC `live_page_text_query_returns_only_matching_lines_with_context`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_page_text_query_returns_only_matching_lines_with_context() {
    if !live_tests_enabled() {
        eprintln!(
            "live_page_text_query_returns_only_matching_lines_with_context: set FF_RDP_LIVE_TESTS=1"
        );
        return;
    }
    let ff = firefox_with_daemon("live_page_text_query_returns_only_matching_lines_with_context");
    let port = ff.port();
    let Some(server) = FixtureServer::start(needle_fixture()) else {
        eprintln!(
            "live_page_text_query_returns_only_matching_lines_with_context: no fixture HTTP — skipping"
        );
        stop_daemon(port);
        return;
    };

    run_json(port, &["navigate", &server.base_url()]);

    let out = run_json(port, &["page-text", "--query", "needle"]);
    let text = out["results"]
        .as_str()
        .unwrap_or_else(|| panic!("results must be the excerpt string: {out}"));
    assert_eq!(
        text.lines().count(),
        5,
        "the match plus ±2 lines of context: {out}"
    );
    assert!(
        text.contains("the needle is here"),
        "the match itself must be present: {out}"
    );
    assert!(
        !text.contains("filler line 1\n"),
        "lines far from the match must be gone: {out}"
    );
    assert_eq!(out["meta"]["matches"], 1, "one matching line: {out}");
    assert_eq!(out["meta"]["shown"], 1, "nothing was cut: {out}");
    assert_eq!(
        out["meta"]["context_lines"], 2,
        "the default context: {out}"
    );
    assert_eq!(
        out["meta"]["match_lines"],
        serde_json::json!([40]),
        "the needle sits on line 40 of 60: {out}"
    );

    // The excerpt is a fraction of the page, and `meta.total_chars` still
    // reports the whole document so the caller can judge the trade.
    let total = out["meta"]["total_chars"]
        .as_u64()
        .unwrap_or_else(|| panic!("meta.total_chars must be a number: {out}"));
    assert!(
        (text.chars().count() as u64) < total,
        "the excerpt must be smaller than the page ({} vs {total}): {out}",
        text.chars().count()
    );

    // `--context 0` narrows to the match alone.
    let tight = run_json(port, &["page-text", "--query", "needle", "--context", "0"]);
    assert_eq!(
        tight["results"], "the needle is here",
        "--context 0 keeps only the match: {tight}"
    );

    stop_daemon(port);
}

/// AC `live_page_text_is_capped_by_default`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_page_text_is_capped_by_default() {
    if !live_tests_enabled() {
        eprintln!("live_page_text_is_capped_by_default: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_page_text_is_capped_by_default");
    let port = ff.port();
    let Some(server) = FixtureServer::start(needle_fixture()) else {
        eprintln!("live_page_text_is_capped_by_default: no fixture HTTP — skipping");
        stop_daemon(port);
        return;
    };

    run_json(port, &["navigate", &format!("{}/long", server.base_url())]);

    let capped = run_json(port, &["page-text"]);
    let text = capped["results"]
        .as_str()
        .unwrap_or_else(|| panic!("results must be a string: {capped}"));
    assert!(
        text.chars().count() <= 8_000,
        "the default cap is 8000 chars, got {}: {capped}",
        text.chars().count()
    );
    assert_eq!(
        capped["meta"]["truncated"], true,
        "a 20 000-char page must report truncation: {capped}"
    );
    assert_eq!(
        capped["meta"]["total_chars"], 20_000,
        "total_chars must report the whole page, not the excerpt: {capped}"
    );
    let hint = capped["hint"]
        .as_str()
        .unwrap_or_else(|| panic!("a truncated response must carry a hint: {capped}"));
    assert!(
        hint.contains("--full") && hint.contains("--query"),
        "the hint must name the escape hatches: {hint}"
    );

    let full = run_json(port, &["page-text", "--full"]);
    let full_text = full["results"]
        .as_str()
        .unwrap_or_else(|| panic!("results must be a string: {full}"));
    assert_eq!(
        full_text.chars().count(),
        20_000,
        "--full must lift the cap: {}",
        full["meta"]
    );
    assert_eq!(full["meta"]["truncated"], false, "{}", full["meta"]);
    assert!(
        full.get("hint").is_none(),
        "an untruncated response must not claim truncation: {}",
        full["meta"]
    );

    // `--max-chars 0` is rejected before the browser round-trip.
    let zero = run(port, &["page-text", "--max-chars", "0"]);
    assert!(
        !zero.status.success(),
        "--max-chars 0 must fail: {}",
        String::from_utf8_lossy(&zero.stdout)
    );
    let stderr = String::from_utf8_lossy(&zero.stderr);
    assert!(
        stderr.contains("--max-chars"),
        "the error must name the flag: {stderr}"
    );

    stop_daemon(port);
}

// ---------------------------------------------------------------------------
// Theme A — `snapshot --query`
// ---------------------------------------------------------------------------

/// AC `live_snapshot_query_keeps_ancestors_of_matches`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_snapshot_query_keeps_ancestors_of_matches() {
    if !live_tests_enabled() {
        eprintln!("live_snapshot_query_keeps_ancestors_of_matches: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_snapshot_query_keeps_ancestors_of_matches");
    let port = ff.port();
    let Some(server) = FixtureServer::start(table_fixture()) else {
        eprintln!("live_snapshot_query_keeps_ancestors_of_matches: no fixture HTTP — skipping");
        stop_daemon(port);
        return;
    };

    run_json(port, &["navigate", &server.base_url()]);

    let out = run_json(port, &["snapshot", "--query", "1804"]);
    let tree = &out["results"];
    assert_eq!(tree["tag"], "html", "the root must still be html: {out}");
    assert_eq!(out["meta"]["matches"], 1, "one matching cell: {out}");

    let chain = only_child_chain(tree);
    assert_eq!(
        chain.first().map(String::as_str),
        Some("html"),
        "chain {chain:?}: {out}"
    );
    assert_eq!(
        chain.last().map(String::as_str),
        Some("td"),
        "the leaf must be the matching cell, chain {chain:?}: {out}"
    );
    assert!(
        chain.iter().any(|t| t == "table"),
        "the path through the table must be kept, chain {chain:?}: {out}"
    );

    // The sibling row and the unrelated heading are pruned away entirely.
    let serialized = serde_json::to_string(tree).expect("tree must serialize");
    assert!(
        serialized.contains("1804"),
        "the match must survive: {serialized}"
    );
    assert!(
        !serialized.contains("1927"),
        "the non-matching row must be pruned: {serialized}"
    );
    assert!(
        !serialized.contains("World population"),
        "the non-matching heading must be pruned: {serialized}"
    );

    // A query nothing matches yields an empty result, not the whole page.
    let empty = run_json(port, &["snapshot", "--query", "no-such-token-211"]);
    assert_eq!(empty["meta"]["matches"], 0, "{empty}");
    assert_eq!(empty["results"], Value::Null, "{empty}");

    stop_daemon(port);
}

// ---------------------------------------------------------------------------
// Theme A — `a11y summary --query`
// ---------------------------------------------------------------------------

/// AC `live_a11y_summary_query_filters_and_keeps_refs`: the survivors all
/// match, and each still carries a `ref` that `click --ref` accepts.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_a11y_summary_query_filters_and_keeps_refs() {
    if !live_tests_enabled() {
        eprintln!("live_a11y_summary_query_filters_and_keeps_refs: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_a11y_summary_query_filters_and_keeps_refs");
    let port = ff.port();
    let Some(server) = FixtureServer::start(names_fixture()) else {
        eprintln!("live_a11y_summary_query_filters_and_keeps_refs: no fixture HTTP — skipping");
        stop_daemon(port);
        return;
    };

    run_json(port, &["navigate", &server.base_url()]);

    let out = run_json(port, &["a11y", "summary", "--query", "Babbage"]);
    assert_eq!(
        out["meta"]["refs_registered"], true,
        "the daemon route must still register refs under --query: {out}"
    );
    let interactive = out["results"]["interactive"]
        .as_array()
        .unwrap_or_else(|| panic!("results.interactive must be an array: {out}"));
    assert_eq!(interactive.len(), 1, "only the Babbage link matches: {out}");
    let entry = &interactive[0];
    assert_eq!(entry["name"], "Charles Babbage", "{out}");
    let ref_id = entry["ref"]
        .as_str()
        .unwrap_or_else(|| panic!("a filtered survivor must keep its ref: {out}"))
        .to_owned();

    // Every other section is filtered by the same predicate.
    for section in ["landmarks", "headings"] {
        let entries = out["results"][section]
            .as_array()
            .unwrap_or_else(|| panic!("results.{section} must be an array: {out}"));
        assert!(
            entries.is_empty(),
            "nothing in {section} mentions Babbage: {out}"
        );
    }
    assert_eq!(out["meta"]["matches"], 1, "{out}");

    // The ref is real, not decoration: it clicks.
    let click = run_json(port, &["click", "--ref", &ref_id]);
    assert_eq!(
        click["results"]["clicked"], true,
        "click --ref {ref_id} must work on a filtered entry: {click}"
    );

    stop_daemon(port);
}

// ---------------------------------------------------------------------------
// Theme C — full accessible names
// ---------------------------------------------------------------------------

/// AC `live_dom_text_returns_full_accessible_name`: `<h3><a>Bug:
/// <span>title</span></a></h3>` → `dom "h3 a" --text` yields `"Bug: title"`,
/// the shape that made the benchmark's `github_issue_investigation` report
/// four bare `Bug:` labels as the issue titles.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_dom_text_returns_full_accessible_name() {
    if !live_tests_enabled() {
        eprintln!("live_dom_text_returns_full_accessible_name: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_dom_text_returns_full_accessible_name");
    let port = ff.port();
    let Some(server) = FixtureServer::start(names_fixture()) else {
        eprintln!("live_dom_text_returns_full_accessible_name: no fixture HTTP — skipping");
        stop_daemon(port);
        return;
    };

    run_json(port, &["navigate", &server.base_url()]);

    let out = run_json(port, &["dom", "h3 a", "--text"]);
    assert_eq!(
        out["results"][0], "Bug: title",
        "the whole accessible name, not the first text node: {out}"
    );

    // The ARIA-tree `name` field agrees with `--text`.
    let aria = run_json(port, &["dom", "h3 a"]);
    assert_eq!(
        aria["results"][0]["name"], "Bug: title",
        "the ARIA-tree name must be the same accessible name: {aria}"
    );

    // And `--query` reaches it — the whole point of computing it in full.
    let queried = run_json(port, &["dom", "a", "--query", "title"]);
    assert_eq!(queried["meta"]["matches"], 1, "{queried}");
    assert_eq!(queried["results"][0]["name"], "Bug: title", "{queried}");

    stop_daemon(port);
}
