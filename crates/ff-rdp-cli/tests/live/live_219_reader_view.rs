//! Live tests for iteration 219 — reader view on the live page.
//!
//! The 2026-08-30 re-measurement in `kb/research/axi-benchmark-comparison.md`
//! found that `--with-page` did not shorten the click-through tasks even when
//! agents used it, for two reasons this iteration fixes:
//!
//! * `interactive` was the first 50 links in DOM order, which on a real page is
//!   all site chrome — the article's own links sat behind
//!   `interactive_truncated: true`, so `click --ref` was useless exactly where
//!   it mattered; and
//! * the view carried no text at all, so the agent fetched `page-text` anyway.
//!
//! Mozilla's `Readability.js` now runs on the live page (Theme A/B/D) and its
//! article element decides `zone: "content" | "chrome"`; content sorts before
//! chrome so the 50-entry cap falls on the navigation bar, and the article's
//! text becomes `page.excerpt` (Theme C).
//!
//! The fixture deliberately mirrors `en.wikipedia.org/wiki/Ada_Lovelace`: a
//! long navigation bar, an `<article>` whose lede names Charles Babbage, and a
//! destination page whose lede carries his birth year — the exact shape of the
//! benchmark's `wikipedia_link_follow` task.
//!
//! daemon-parity: every test uses the daemon route (no `--no-daemon`), because
//! the daemon owns the ref store and a page view without usable refs proves
//! the opposite of the point.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live live_219_reader_view -- --nocapture

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

/// The lede of the origin page — the text `page.excerpt` must open with.
const ADA_LEDE: &str = "Augusta Ada King, Countess of Lovelace was an English mathematician \
     and writer, chiefly known for her work on Charles Babbage's proposed mechanical \
     general-purpose computer, the analytical engine.";

/// A Wikipedia-shaped two-page fixture.
///
/// 80 navigation links precede the article, so the article's own link sits at
/// DOM position 81 — past the 50-entry cap that iter-210 applied in DOM order.
/// That is the defect this iteration exists to fix, reproduced in miniature.
fn wiki_shaped_fixture() -> HashMap<String, FixtureRoute> {
    let mut nav = String::from("<a href=\"#content\">Jump to content</a>");
    for i in 0..80 {
        let _ = write!(nav, "<a href=\"/nav/{i}\">Nav item {i}</a>");
    }
    let mut routes = HashMap::new();
    routes.insert(
        "/".to_owned(),
        FixtureRoute::html(format!(
            "<!doctype html><html><head><title>Ada Lovelace</title></head><body>\
             <nav>{nav}</nav>\
             <main><article><h1>Ada Lovelace</h1>\
             <p>{ADA_LEDE}</p>\
             <p>She was the first to recognise that the machine had applications beyond \
             pure calculation, and published the first algorithm intended to be carried \
             out by such a machine. She corresponded with \
             <a href=\"/babbage\">Charles Babbage</a> for years about the design.</p>\
             <p>Her notes on the engine include what is recognised as the first computer \
             program, and her reputation grew steadily through the twentieth century as \
             computing itself became a discipline.</p>\
             </article></main>\
             <footer><a href=\"/privacy\">Privacy policy</a></footer></body></html>"
        )),
    );
    routes.insert(
        "/babbage".to_owned(),
        FixtureRoute::html(
            "<!doctype html><html><head><title>Charles Babbage</title></head><body>\
             <nav><a href=\"/\">Home</a></nav>\
             <main><article><h1>Charles Babbage</h1>\
             <p>Charles Babbage (26 December 1791 - 18 October 1871) was an English \
             polymath, mathematician, philosopher and mechanical engineer who originated \
             the concept of a digital programmable computer.</p>\
             <p>He is considered by some to be the father of the computer, and he is \
             credited with inventing the first mechanical computer, the difference \
             engine, that eventually led to more complex electronic designs.</p>\
             </article></main></body></html>",
        ),
    );
    routes
}

/// A page with no prose at all — the Readability-returns-null path.
fn form_only_fixture() -> HashMap<String, FixtureRoute> {
    let mut routes = HashMap::new();
    routes.insert(
        "/".to_owned(),
        FixtureRoute::html(
            "<!doctype html><html><head><title>Sign in</title></head><body>\
             <nav><a href=\"/help\">Help</a></nav>\
             <main><h1>Sign in</h1>\
             <label for=\"u\">Username</label><input id=\"u\" name=\"username\">\
             <label for=\"p\">Password</label><input id=\"p\" type=\"password\" name=\"password\">\
             <button>Continue</button></main></body></html>",
        ),
    );
    routes
}

/// A page that replaces the built-ins the collector would otherwise trust.
///
/// `Array.prototype.forEach` returns nothing, `JSON.stringify` lies, and
/// `Object.prototype` gains an enumerable property — the three ways a page can
/// corrupt a naive in-page collector. Theme D's answer is that the collector
/// uses none of them.
fn hostile_fixture() -> HashMap<String, FixtureRoute> {
    let mut routes = HashMap::new();
    routes.insert(
        "/".to_owned(),
        FixtureRoute::html(
            "<!doctype html><html><head><title>Hostile</title>\
             <script>\
             Array.prototype.forEach = function() { return undefined; };\
             Array.prototype.push = function() { return 0; };\
             JSON.stringify = function() { return '\"tampered\"'; };\
             Object.defineProperty(Object.prototype, 'injected', \
               {value: 'boom', enumerable: true, configurable: true});\
             </script></head><body>\
             <nav><a href=\"/nav\">Site nav</a></nav>\
             <main><article><h1>Hostile page</h1>\
             <p>This paragraph is the article body and must reach the excerpt intact, \
             even though the page replaced every array and JSON built-in an in-page \
             collector might reasonably have reached for.</p>\
             <p>A second paragraph follows so Readability has enough text to score the \
             article as content rather than as boilerplate chrome.</p>\
             <a href=\"/deep\">Deep link</a>\
             <span id=\"hostile-label\" hidden>Labelled link</span>\
             <a href=\"/labelled\" aria-labelledby=\"hostile-label\">x</a>\
             </article></main></body></html>",
        ),
    );
    routes
}

/// The `interactive` entry with this `name`, if it survived the cap.
fn entry_named<'a>(page: &'a Value, name: &str) -> Option<&'a Value> {
    page["interactive"]
        .as_array()?
        .iter()
        .find(|e| e["name"] == name)
}

// ---------------------------------------------------------------------------
// Theme B — zones, ordering, and the cap
// ---------------------------------------------------------------------------

/// AC 1: the article's link is inside the 50-entry cap with `zone: "content"`,
/// the navigation is not, and `excerpt` opens with the lede.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_219_content_links_outrank_chrome_in_the_capped_view() {
    if !live_tests_enabled() {
        eprintln!(
            "live_219_content_links_outrank_chrome_in_the_capped_view: set FF_RDP_LIVE_TESTS=1"
        );
        return;
    }
    let ff = firefox_with_daemon("live_219_content_links_outrank_chrome_in_the_capped_view");
    let port = ff.port();
    let Some(server) = FixtureServer::start(wiki_shaped_fixture()) else {
        eprintln!("live_219_content_links_outrank_chrome_in_the_capped_view: no fixture HTTP");
        stop_daemon(port);
        return;
    };

    let nav = run_json(port, &["navigate", &server.base_url(), "--with-page"]);
    let page = &nav["results"]["page"];

    let babbage = entry_named(page, "Charles Babbage").unwrap_or_else(|| {
        panic!("the article's own link must survive the cap — that is the whole point: {nav}")
    });
    assert_eq!(
        babbage["zone"], "content",
        "a link inside <article> is content: {nav}"
    );
    let ref_id = babbage["ref"]
        .as_str()
        .unwrap_or_else(|| panic!("a content entry must carry a usable ref: {nav}"));
    assert!(ref_id.starts_with('e'), "ref {ref_id:?}: {nav}");

    let entries = page["interactive"]
        .as_array()
        .unwrap_or_else(|| panic!("page.interactive must be an array: {nav}"));
    assert_eq!(entries.len(), 50, "the cap is unchanged at 50: {nav}");
    assert_eq!(
        entries[0]["name"], "Charles Babbage",
        "content leads the list — the cap must fall on the nav, not the article: {nav}"
    );
    // Ordering, not membership: this fixture has one content link and 81 nav
    // links, so 49 chrome entries legitimately fill the rest of the cap. What
    // must never happen is a content entry sorted *after* a chrome one.
    let first_chrome = entries
        .iter()
        .position(|e| e["zone"] == "chrome")
        .unwrap_or(entries.len());
    assert!(
        entries[first_chrome..]
            .iter()
            .all(|e| e["zone"] == "chrome"),
        "no content entry may sort after a chrome one: {nav}"
    );
    assert!(
        entry_named(page, "Nav item 79").is_none(),
        "the cap must drop the tail of the navigation bar: {nav}"
    );
    assert_eq!(page["interactive_truncated"], true, "{nav}");
    let omitted = page["chrome_omitted"]
        .as_u64()
        .unwrap_or_else(|| panic!("chrome_omitted must report the nav the cap dropped: {nav}"));
    assert!(omitted > 0, "81 nav links, 50-entry cap: {nav}");

    // …and the view carries text now, starting at the lede.
    assert_eq!(page["source"], "readability", "{nav}");
    assert_eq!(page["readerable"], true, "{nav}");
    let excerpt = page["excerpt"]
        .as_str()
        .unwrap_or_else(|| panic!("page.excerpt must be present: {nav}"));
    assert!(
        excerpt.starts_with("Augusta Ada King"),
        "the excerpt must open with the lede, not the nav: {excerpt:?}"
    );
    assert!(
        !excerpt.contains("Nav item"),
        "the navigation is not article text: {excerpt:?}"
    );
    assert_eq!(
        page["excerpt_chars"].as_u64(),
        Some(excerpt.chars().count() as u64),
        "excerpt_chars must describe the excerpt actually returned: {nav}"
    );
    assert!(
        nav["meta"]["page_parse_ms"].as_f64().is_some(),
        "meta.page_parse_ms must report the in-content parse cost: {nav}"
    );
    assert_eq!(
        nav["meta"]["page_readability_injected"], true,
        "a fresh document has to be given the bundle: {nav}"
    );
    assert!(
        page.get("landmarks").is_none(),
        "iter-219 Theme B drops landmarks from the act-and-see view: {nav}"
    );

    stop_daemon(port);
}

/// AC 2: `click --ref` on that link returns the destination page's text, so
/// `wikipedia_link_follow` is answerable in two commands rather than four.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_219_click_ref_with_page_returns_the_destination_text() {
    if !live_tests_enabled() {
        eprintln!(
            "live_219_click_ref_with_page_returns_the_destination_text: set FF_RDP_LIVE_TESTS=1"
        );
        return;
    }
    let ff = firefox_with_daemon("live_219_click_ref_with_page_returns_the_destination_text");
    let port = ff.port();
    let Some(server) = FixtureServer::start(wiki_shaped_fixture()) else {
        eprintln!("live_219_click_ref_with_page_returns_the_destination_text: no fixture HTTP");
        stop_daemon(port);
        return;
    };

    let nav = run_json(port, &["navigate", &server.base_url(), "--with-page"]);
    let ref_id = entry_named(&nav["results"]["page"], "Charles Babbage")
        .and_then(|e| e["ref"].as_str())
        .unwrap_or_else(|| panic!("no ref for the Babbage link: {nav}"))
        .to_owned();

    let click = run_json(port, &["click", "--ref", &ref_id, "--with-page"]);
    let page = &click["results"]["page"];
    assert_eq!(
        page["headings"][0]["text"], "Charles Babbage",
        "the view must describe the DESTINATION page: {click}"
    );
    let excerpt = page["excerpt"]
        .as_str()
        .unwrap_or_else(|| panic!("the destination view must carry text: {click}"));
    assert!(
        excerpt.contains("1791"),
        "the birth year is the task's answer and must be in the returned page: {excerpt:?}"
    );

    stop_daemon(port);
}

// ---------------------------------------------------------------------------
// Theme B/D — the live DOM is not touched
// ---------------------------------------------------------------------------

/// AC 3: collection stamps `data-ffrdp-id` on every interactive element and
/// strips it again in a `finally`, so the document is byte-identical after a
/// `--with-page` call. `--with-page`'s promise since iter-210 is that looking
/// at the page does not change it.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_219_collection_leaves_the_dom_byte_identical() {
    if !live_tests_enabled() {
        eprintln!("live_219_collection_leaves_the_dom_byte_identical: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_219_collection_leaves_the_dom_byte_identical");
    let port = ff.port();
    let Some(server) = FixtureServer::start(wiki_shaped_fixture()) else {
        eprintln!("live_219_collection_leaves_the_dom_byte_identical: no fixture HTTP");
        stop_daemon(port);
        return;
    };
    run_json(port, &["navigate", &server.base_url()]);

    let before = run_json(
        port,
        &[
            "eval",
            "--stringify",
            "document.documentElement.outerHTML.length + ':' + \
             document.querySelectorAll('[data-ffrdp-id]').length",
        ],
    );
    run_json(port, &["scroll", "top", "--with-page"]);
    let after = run_json(
        port,
        &[
            "eval",
            "--stringify",
            "document.documentElement.outerHTML.length + ':' + \
             document.querySelectorAll('[data-ffrdp-id]').length",
        ],
    );

    assert_eq!(
        before["results"], after["results"],
        "the live DOM must be unchanged by collection: {before} vs {after}"
    );
    assert!(
        after["results"].as_str().is_some_and(|s| s.ends_with(":0")),
        "no data-ffrdp-id attribute may survive collection: {after}"
    );

    stop_daemon(port);
}

/// Theme D: the ~32 KB bundle is shipped once per document, not once per call.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_219_readability_is_injected_once_per_document() {
    if !live_tests_enabled() {
        eprintln!("live_219_readability_is_injected_once_per_document: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_219_readability_is_injected_once_per_document");
    let port = ff.port();
    let Some(server) = FixtureServer::start(wiki_shaped_fixture()) else {
        eprintln!("live_219_readability_is_injected_once_per_document: no fixture HTTP");
        stop_daemon(port);
        return;
    };

    let first = run_json(port, &["navigate", &server.base_url(), "--with-page"]);
    assert_eq!(
        first["meta"]["page_readability_injected"], true,
        "a freshly-loaded document has no bundle yet: {first}"
    );
    let second = run_json(port, &["scroll", "top", "--with-page"]);
    assert_eq!(
        second["meta"]["page_readability_injected"], false,
        "the second call on the same document must reuse the cached handle: {second}"
    );
    assert_eq!(
        second["results"]["page"]["source"], "readability",
        "…and still produce a reader view: {second}"
    );

    stop_daemon(port);
}

/// Theme D: a page that replaced `JSON.stringify`, `Array.prototype.push` and
/// `Array.prototype.forEach`, and added an enumerable property to
/// `Object.prototype`, still gets a correct view — the collector serialises
/// with its own writer and never calls a method the page can swap.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_219_hostile_builtins_do_not_corrupt_the_view() {
    if !live_tests_enabled() {
        eprintln!("live_219_hostile_builtins_do_not_corrupt_the_view: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_219_hostile_builtins_do_not_corrupt_the_view");
    let port = ff.port();
    let Some(server) = FixtureServer::start(hostile_fixture()) else {
        eprintln!("live_219_hostile_builtins_do_not_corrupt_the_view: no fixture HTTP");
        stop_daemon(port);
        return;
    };

    let nav = run_json(port, &["navigate", &server.base_url(), "--with-page"]);
    let page = &nav["results"]["page"];
    assert_eq!(
        page["headings"][0]["text"], "Hostile page",
        "the heading must survive a page that replaced the array built-ins: {nav}"
    );
    let excerpt = page["excerpt"]
        .as_str()
        .unwrap_or_else(|| panic!("the excerpt must survive too: {nav}"));
    assert!(
        excerpt.contains("article body"),
        "the article text must be intact, not 'tampered': {excerpt:?}"
    );
    assert!(
        entry_named(page, "Deep link").is_some(),
        "the in-article link must still be collected: {nav}"
    );
    // iter-219 review: the aria-labelledby branch of __ffrdpAccName is the one
    // acc-name path that used the patched built-ins — exercise it explicitly.
    assert!(
        entry_named(page, "Labelled link").is_some(),
        "an aria-labelledby name must survive the patched built-ins: {nav}"
    );

    stop_daemon(port);
}

// ---------------------------------------------------------------------------
// Theme C — excerpt, fallback, --page-chars, --query
// ---------------------------------------------------------------------------

/// Theme C: a page with no prose is exactly where Readability returns `null`.
/// The view must still carry text — from `main`'s `innerText` — and say so.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_219_prose_free_page_falls_back_to_innertext() {
    if !live_tests_enabled() {
        eprintln!("live_219_prose_free_page_falls_back_to_innertext: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_219_prose_free_page_falls_back_to_innertext");
    let port = ff.port();
    let Some(server) = FixtureServer::start(form_only_fixture()) else {
        eprintln!("live_219_prose_free_page_falls_back_to_innertext: no fixture HTTP");
        stop_daemon(port);
        return;
    };

    let nav = run_json(port, &["navigate", &server.base_url(), "--with-page"]);
    let page = &nav["results"]["page"];
    let source = page["source"]
        .as_str()
        .unwrap_or_else(|| panic!("page.source must always be present: {nav}"));
    assert!(
        source == "innertext" || source == "readability",
        "page.source must name one of the two extractors, got {source:?}: {nav}"
    );
    let excerpt = page["excerpt"]
        .as_str()
        .unwrap_or_else(|| panic!("a page with visible text must never return no excerpt: {nav}"));
    assert!(
        excerpt.contains("Sign in"),
        "the fallback must return the page's own text: {excerpt:?}"
    );
    assert_eq!(
        page["readerable"], false,
        "a sign-in form is not an article: {nav}"
    );
    // The inputs are still collected and still get refs.
    assert!(
        entry_named(page, "Username").is_some(),
        "the form fields must be in the view: {nav}"
    );

    stop_daemon(port);
}

/// Theme C: `--page-chars` sizes the excerpt and `0` turns it off entirely
/// (the documented "structure only" knob), without disturbing the zones.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_219_page_chars_sizes_and_disables_the_excerpt() {
    if !live_tests_enabled() {
        eprintln!("live_219_page_chars_sizes_and_disables_the_excerpt: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_219_page_chars_sizes_and_disables_the_excerpt");
    let port = ff.port();
    let Some(server) = FixtureServer::start(wiki_shaped_fixture()) else {
        eprintln!("live_219_page_chars_sizes_and_disables_the_excerpt: no fixture HTTP");
        stop_daemon(port);
        return;
    };
    let url = server.base_url();

    let small = run_json(
        port,
        &["navigate", &url, "--with-page", "--page-chars", "200"],
    );
    let page = &small["results"]["page"];
    let excerpt = page["excerpt"].as_str().unwrap_or_default();
    assert!(
        excerpt.chars().count() <= 200,
        "--page-chars 200 must be respected, got {}: {small}",
        excerpt.chars().count()
    );
    assert_eq!(
        page["excerpt_truncated"], true,
        "the fixture article is longer than 200 chars: {small}"
    );
    assert!(
        !excerpt.ends_with("Charles Babb"),
        "the cut must land on a boundary, never mid-word: {excerpt:?}"
    );

    let structure_only = run_json(
        port,
        &["navigate", &url, "--with-page", "--page-chars", "0"],
    );
    let page = &structure_only["results"]["page"];
    assert!(
        page.get("excerpt").is_none(),
        "--page-chars 0 is the structure-only knob: {structure_only}"
    );
    assert_eq!(
        entry_named(page, "Charles Babbage").map(|e| e["zone"].clone()),
        Some(Value::from("content")),
        "…but the zones and the ordering still apply: {structure_only}"
    );

    stop_daemon(port);
}

/// Theme C: `--query` narrows both halves of the view — the excerpt to the
/// match window and `interactive` to the entries whose name or href match —
/// with `page.matches` counting the survivors.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_219_query_narrows_the_embedded_page_view() {
    if !live_tests_enabled() {
        eprintln!("live_219_query_narrows_the_embedded_page_view: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_219_query_narrows_the_embedded_page_view");
    let port = ff.port();
    let Some(server) = FixtureServer::start(wiki_shaped_fixture()) else {
        eprintln!("live_219_query_narrows_the_embedded_page_view: no fixture HTTP");
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
            "Babbage",
        ],
    );
    let page = &nav["results"]["page"];
    let interactive = page["interactive"]
        .as_array()
        .unwrap_or_else(|| panic!("page.interactive must be an array: {nav}"));
    assert!(!interactive.is_empty(), "one link matches: {nav}");
    for entry in interactive {
        let name = entry["name"].as_str().unwrap_or_default();
        let href = entry["href"].as_str().unwrap_or_default();
        assert!(
            name.to_lowercase().contains("babbage") || href.to_lowercase().contains("babbage"),
            "every survivor must match the query: {entry} in {nav}"
        );
    }
    // iter-225 widened `matches`: it counts matching excerpt lines as well as
    // matching entries. Under iter-219 it counted entries only, which reported
    // `matches: 0` beside a perfectly good excerpt window whenever the hit was
    // in the prose — the signal an agent reads to decide whether to spend
    // another turn. So the entry count is now a lower bound, not the total.
    assert!(
        page["matches"].as_u64().unwrap_or(0) >= interactive.len() as u64,
        "every surviving entry must be counted: {nav}"
    );
    assert_eq!(
        page["query_source"].as_str(),
        Some("readability"),
        "the hit is in the article text, and the view must attribute it there: {nav}"
    );
    let excerpt = page["excerpt"].as_str().unwrap_or_default();
    assert!(
        excerpt.contains("Babbage"),
        "the excerpt must be the window around the match: {excerpt:?}"
    );

    // `--query` reaches past the cap: "Nav item 79" is entry 80 in DOM order
    // and is dropped by the 50-entry cap in the unfiltered view.
    let nav_query = run_json(
        port,
        &[
            "navigate",
            &server.base_url(),
            "--with-page",
            "--query",
            "Nav item 79",
        ],
    );
    assert!(
        entry_named(&nav_query["results"]["page"], "Nav item 79").is_some(),
        "--query must reach a control the cap dropped: {nav_query}"
    );

    stop_daemon(port);
}
