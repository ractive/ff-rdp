//! Live tests for iteration 225 — the facts pass and the `--query` fallback.
//!
//! # The defect
//!
//! The 2026-08-31 two-task re-measurement in
//! `kb/research/axi-benchmark-comparison.md` still read 7.7 turns on
//! `wikipedia_link_follow` and 10.3 on `wikipedia_infobox_hop` against axi's
//! 4.0/4.0 — not because agents failed to find `--with-page` (all six runs used
//! it) but because the fact the task asked for was not in the view. On
//! `en.wikipedia.org/wiki/Python_(programming_language)`, `--with-page
//! --page-chars 4000` never contained "Stable release": Readability scores
//! `table.infobox` as boilerplate, correctly for reading and wrongly for
//! answering. Every run then paid 2–6 `page-text --query` round trips on a page
//! it had already fetched.
//!
//! # What these tests pin
//!
//! * `page.facts` carries the infobox rows Readability threw away, in document
//!   order, capped, with `facts_total` reporting the real count (Theme A).
//! * `--query` reaches those rows, and `page.query_source` says so (Theme A).
//! * A query that misses both the article text and the facts falls back to a
//!   window over the page's rendered text rather than returning nothing, and a
//!   query that misses everywhere says so with a `hint` (Theme B).
//! * The facts pass reads and never writes: the live DOM is byte-identical
//!   across a `--with-page` call, exactly as iter-219 requires.
//!
//! The fixture mirrors the Python article's shape: a `table.infobox` whose
//! "Stable release" row exists nowhere in the prose, a definition list, and an
//! `[itemprop]` microdata element.
//!
//! daemon-parity: every test uses the daemon route (the default), like the
//! iter-219/220/224 suites it extends.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live live_225_reader_facts -- --nocapture

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

/// A Wikipedia-article-shaped page: an infobox whose rows appear nowhere in
/// the prose, a definition list, and one microdata element.
///
/// "Stable release" and "3.13.5" are deliberately absent from every paragraph,
/// so a test that finds them proves the facts pass found them — not the
/// excerpt.
fn infobox_fixture() -> HashMap<String, FixtureRoute> {
    let mut routes = HashMap::new();
    routes.insert(
        "/".to_owned(),
        FixtureRoute::html(
            "<!doctype html><html><head><title>Python (programming language)</title></head>\
             <body>\
             <nav><a href=\"/\">Main page</a><a href=\"/help\">Help</a></nav>\
             <main><article><h1>Python (programming language)</h1>\
             <table class=\"infobox\">\
             <tr><th colspan=\"2\">Python</th></tr>\
             <tr><th>Paradigm</th><td>multi-paradigm: object-oriented, procedural</td></tr>\
             <tr><th>Designed by</th><td>Guido van Rossum</td></tr>\
             <tr><th>Stable release</th><td>3.13.5 / 11 June 2025</td></tr>\
             <tr><th>Typing discipline</th><td>duck, dynamic, gradual</td></tr>\
             </table>\
             <p>Python is a high-level, general-purpose programming language whose \
             design philosophy emphasizes code readability with the use of significant \
             indentation. It is dynamically type-checked and garbage-collected.</p>\
             <p>Python consistently ranks as one of the most popular programming \
             languages, and it supports multiple programming paradigms including \
             structured, object-oriented and functional programming.</p>\
             <dl><dt>Filename extensions</dt><dd>.py</dd><dd>.pyw</dd>\
             <dt>Influenced by</dt><dd>ABC, Modula-3, Perl</dd></dl>\
             <p itemprop=\"license\">Python Software Foundation License</p>\
             </article></main>\
             <footer><a href=\"/privacy\">Privacy policy</a></footer></body></html>",
        ),
    );
    routes
}

/// Theme A, and acceptance criterion 1: the infobox row Readability discards
/// comes back under `page.facts`, and the excerpt still does not contain it —
/// which is what makes the facts pass necessary rather than decorative.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_225_facts_carry_the_infobox_rows_the_excerpt_drops() {
    if !live_tests_enabled() {
        eprintln!(
            "live_225_facts_carry_the_infobox_rows_the_excerpt_drops: set FF_RDP_LIVE_TESTS=1"
        );
        return;
    }
    let ff = firefox_with_daemon("live_225_facts_carry_the_infobox_rows_the_excerpt_drops");
    let port = ff.port();
    let Some(server) = FixtureServer::start(infobox_fixture()) else {
        eprintln!("live_225_facts_carry_the_infobox_rows_the_excerpt_drops: no fixture HTTP");
        stop_daemon(port);
        return;
    };

    let nav = run_json(
        port,
        &[
            "navigate",
            &server.base_url(),
            "--with-page",
            "--page-chars",
            "4000",
        ],
    );
    let page = &nav["results"]["page"];

    let facts = page["facts"]
        .as_array()
        .unwrap_or_else(|| panic!("page.facts must be an array: {nav}"));
    let release = facts
        .iter()
        .find(|f| f["key"].as_str() == Some("Stable release"))
        .unwrap_or_else(|| panic!("the infobox row must be a fact: {facts:?} in {nav}"));
    assert!(
        release["value"]
            .as_str()
            .unwrap_or_default()
            .contains("3.13.5"),
        "the fact must carry the value: {release} in {nav}"
    );

    // The whole reason this iteration exists: 4 000 characters of excerpt on a
    // page this short is the entire article, and it still does not have it.
    let excerpt = page["excerpt"].as_str().unwrap_or_default();
    assert!(
        !excerpt.contains("Stable release"),
        "Readability keeps the infobox out of the excerpt — if this ever fails, \
         the fallback story changes: {excerpt:?}"
    );

    // The other two row shapes, from the same document-order pass.
    let keys: Vec<&str> = facts.iter().filter_map(|f| f["key"].as_str()).collect();
    assert!(
        keys.contains(&"Filename extensions"),
        "definition lists are facts too: {keys:?} in {nav}"
    );
    assert!(
        keys.contains(&"license"),
        "[itemprop] microdata is a fact too: {keys:?} in {nav}"
    );
    let ext = facts
        .iter()
        .find(|f| f["key"].as_str() == Some("Filename extensions"))
        .unwrap_or_else(|| panic!("{facts:?}"));
    assert_eq!(
        ext["value"].as_str(),
        Some(".py; .pyw"),
        "every dd after the dt joins the value: {nav}"
    );

    // A row with a `th` but no `td` (the infobox caption) is not a fact.
    assert!(
        !keys.contains(&"Python"),
        "a header-only row has no value and must be skipped: {keys:?}"
    );

    stop_daemon(port);
}

/// Theme A / acceptance criterion 2: `--query` reaches the facts, so the
/// question the benchmark spent a turn on is answered by the navigate itself.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_225_query_answers_from_the_facts_in_one_command() {
    if !live_tests_enabled() {
        eprintln!("live_225_query_answers_from_the_facts_in_one_command: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_225_query_answers_from_the_facts_in_one_command");
    let port = ff.port();
    let Some(server) = FixtureServer::start(infobox_fixture()) else {
        eprintln!("live_225_query_answers_from_the_facts_in_one_command: no fixture HTTP");
        stop_daemon(port);
        return;
    };

    let nav = run_json(
        port,
        &[
            "navigate",
            &server.base_url(),
            "--with-page",
            "--query",
            "Stable release",
        ],
    );
    let page = &nav["results"]["page"];

    assert!(
        page["matches"].as_u64().unwrap_or(0) >= 1,
        "the query must report the hit it found: {nav}"
    );
    assert_eq!(
        page["query_source"].as_str(),
        Some("facts"),
        "the answer came from the infobox, and the view must say so: {nav}"
    );
    let facts = page["facts"]
        .as_array()
        .unwrap_or_else(|| panic!("page.facts must survive the filter: {nav}"));
    assert_eq!(facts.len(), 1, "only the matching row survives: {nav}");
    assert!(
        facts[0]["value"]
            .as_str()
            .unwrap_or_default()
            .contains("3.13.5"),
        "{nav}"
    );

    stop_daemon(port);
}

/// Theme B: a query the reader text and the facts both miss falls back to a
/// window over the page's rendered text — the `page-text --query` round trip
/// the agent would otherwise have spent a turn on, folded into this command.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_225_query_falls_back_to_the_rendered_text() {
    if !live_tests_enabled() {
        eprintln!("live_225_query_falls_back_to_the_rendered_text: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_225_query_falls_back_to_the_rendered_text");
    let port = ff.port();
    let Some(server) = FixtureServer::start(infobox_fixture()) else {
        eprintln!("live_225_query_falls_back_to_the_rendered_text: no fixture HTTP");
        stop_daemon(port);
        return;
    };

    // "Privacy policy" is a footer link: outside the article Readability kept,
    // and not a fact. Before iter-225 this returned matches: 0 and an empty
    // excerpt on a page that plainly contains the words.
    let nav = run_json(
        port,
        &[
            "navigate",
            &server.base_url(),
            "--with-page",
            "--query",
            "Privacy policy",
        ],
    );
    let page = &nav["results"]["page"];
    assert_eq!(
        page["query_source"].as_str(),
        Some("innertext"),
        "the window came from the rendered text and must be labelled so: {nav}"
    );
    let excerpt = page["excerpt"].as_str().unwrap_or_default();
    assert!(
        excerpt.to_lowercase().contains("privacy policy"),
        "the fallback window must carry the match: {excerpt:?} in {nav}"
    );
    assert!(
        page["matches"].as_u64().unwrap_or(0) >= 1,
        "a fallback hit is still a hit: {nav}"
    );
    assert!(page.get("hint").is_none(), "a hit needs no hint: {nav}");

    stop_daemon(port);
}

/// Theme B, the honest half: nothing on the page matches, so the view says
/// nothing matched and names the one command that searches more than it did.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_225_a_miss_everywhere_reports_zero_and_hints() {
    if !live_tests_enabled() {
        eprintln!("live_225_a_miss_everywhere_reports_zero_and_hints: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_225_a_miss_everywhere_reports_zero_and_hints");
    let port = ff.port();
    let Some(server) = FixtureServer::start(infobox_fixture()) else {
        eprintln!("live_225_a_miss_everywhere_reports_zero_and_hints: no fixture HTTP");
        stop_daemon(port);
        return;
    };

    let nav = run_json(
        port,
        &[
            "navigate",
            &server.base_url(),
            "--with-page",
            "--query",
            "zzz-not-on-this-page",
        ],
    );
    let page = &nav["results"]["page"];
    assert_eq!(page["matches"].as_u64(), Some(0), "{nav}");
    assert_eq!(page["excerpt"].as_str(), Some(""), "{nav}");
    assert!(
        page.get("query_source").is_none(),
        "nothing answered, so nothing may be credited: {nav}"
    );
    let hint = page["hint"].as_str().unwrap_or_default();
    assert!(
        hint.contains("page-text --full --query"),
        "the miss must name the exhaustive next step: {hint:?} in {nav}"
    );

    stop_daemon(port);
}

/// The facts pass reads the document and never writes to it — the same
/// byte-identical-DOM property `live_219_reader_view` pins for the stamping
/// reader pass, re-checked now that a second pass walks the same tree.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_225_the_facts_pass_leaves_the_dom_untouched() {
    if !live_tests_enabled() {
        eprintln!("live_225_the_facts_pass_leaves_the_dom_untouched: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_225_the_facts_pass_leaves_the_dom_untouched");
    let port = ff.port();
    let Some(server) = FixtureServer::start(infobox_fixture()) else {
        eprintln!("live_225_the_facts_pass_leaves_the_dom_untouched: no fixture HTTP");
        stop_daemon(port);
        return;
    };

    run_json(port, &["navigate", &server.base_url()]);
    let before = run_json(
        port,
        &["eval", "--stringify", "document.documentElement.outerHTML"],
    );
    run_json(port, &["reload", "--with-page", "--page-chars", "2000"]);
    let after = run_json(
        port,
        &["eval", "--stringify", "document.documentElement.outerHTML"],
    );
    assert_eq!(
        before["results"], after["results"],
        "a --with-page call must leave the document byte-identical"
    );

    stop_daemon(port);
}
