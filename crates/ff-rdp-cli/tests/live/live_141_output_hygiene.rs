//! Live tests for iteration 141 — output hygiene: text padding, invalid
//! JSON, snapshot economics.
//!
//! From [[dogfooding-session-63]]. Covers:
//! - Theme A: `--format text` no longer pads every row to the widest cell.
//! - Theme B: `index` emits exactly one JSON document on stdout, and its
//!   robots.txt parser respects `User-agent:` grouping (a foreign-UA
//!   `Disallow: /` must not block a generic crawl).
//! - Theme C: `snapshot`'s `meta` reports truncation and the effective
//!   bound, rather than burying a `truncated: true` marker inside `results`.
//! - Theme D: an empty `--format text` result still reports sample-size and
//!   capped-state metadata instead of a bare `[]`.
//!
//! daemon-parity: every test here uses [`daemon_args`] (no `--no-daemon`) —
//! the default connection mode is exactly what a real invocation uses, and
//! iteration 137 already established the daemon-parity pattern this suite
//! follows (see `live_140_element_targeting.rs`, which this file's helpers
//! are modeled on).
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live live_141_output_hygiene -- --nocapture

use std::collections::HashMap;
use std::process::{Command, Output};

use serde_json::Value;

use crate::common::{FixtureRoute, FixtureServer, LiveFirefox, ff_rdp_bin, live_tests_enabled};

/// Args for the **default** connection mode: no `--no-daemon`, so the CLI
/// auto-starts and proxies through the daemon — see the module-level
/// `daemon-parity` note.
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

/// Bring up Firefox with a running daemon, or `None` with a printed reason.
fn firefox_with_daemon(test: &str) -> Option<LiveFirefox> {
    let ff = LiveFirefox::headless_on_random_port()?;
    if ff.with_daemon().is_none() {
        eprintln!("{test}: daemon did not start — skipping");
        return None;
    }
    Some(ff)
}

fn navigate(port: u16, url: &str) {
    let nav = Command::new(ff_rdp_bin())
        .args(daemon_args(port))
        .args(["navigate", url])
        .output()
        .expect("ff-rdp navigate");
    assert!(
        nav.status.success(),
        "navigate to {url} failed: {}",
        String::from_utf8_lossy(&nav.stderr)
    );
}

/// Run `ff-rdp <args>` over the daemon connection and return the raw output
/// (caller decides success/failure).
fn run(port: u16, args: &[&str]) -> Output {
    Command::new(ff_rdp_bin())
        .args(daemon_args(port))
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("spawn ff-rdp {args:?}: {e}"))
}

/// Run `ff-rdp <args>`, require success, and parse stdout as JSON.
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

// ---------------------------------------------------------------------------
// Theme A — `--format text` no longer pads every row to the widest cell
// ---------------------------------------------------------------------------

/// AC: `live_141_console_text_bounded` — `console --level error --format
/// text` on a page with a very long message stays bounded; no row padded to
/// another row's width.
///
/// Before the fix, a single ~6000-char console message set the column width
/// for every row — a short message's row got padded out to match it. This
/// asserts both the total output size and every individual line's width stay
/// small, regardless of the long message's real length.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_141_console_text_bounded() {
    if !live_tests_enabled() {
        eprintln!("live_141_console_text_bounded: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let Some(ff) = firefox_with_daemon("live_141_console_text_bounded") else {
        return;
    };
    let port = ff.port();

    let mut routes = HashMap::new();
    routes.insert(
        "/".to_owned(),
        FixtureRoute::html("<!doctype html><title>t141 console</title><body>hi</body>"),
    );
    let Some(server) = FixtureServer::start(routes) else {
        eprintln!("live_141_console_text_bounded: could not bind fixture HTTP — skipping");
        stop_daemon(port);
        return;
    };

    navigate(port, &server.base_url());

    // One short error and one very long one (~6000 chars) — the padding bug
    // only shows up when rows of very different length share a table.
    let long_js =
        "console.error('short'); console.error('x'.repeat(6000)); console.error('also short');";
    let eval = run(port, &["eval", long_js]);
    assert!(
        eval.status.success(),
        "eval failed: {}",
        String::from_utf8_lossy(&eval.stderr)
    );

    let out = run(port, &["console", "--level", "error", "--format", "text"]);
    assert!(
        out.status.success(),
        "console --format text failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);

    // Well under the 255 KB / 8725-column dogfooding regression — a bounded
    // table of 3 short rows should be a few hundred bytes at most.
    assert!(
        stdout.len() < 10_000,
        "console --format text output must stay bounded, got {} bytes:\n{stdout}",
        stdout.len()
    );
    for line in stdout.lines() {
        assert!(
            line.chars().count() < 300,
            "every line must be bounded regardless of the long message's real \
             length — a padded-to-widest-row line would be ~6000+ chars, got \
             {} chars: {line:?}",
            line.chars().count()
        );
    }

    stop_daemon(port);
}

// ---------------------------------------------------------------------------
// Theme B — `index`: single JSON document, robots.txt UA grouping
// ---------------------------------------------------------------------------

/// AC: `live_141_index_single_json_document` — `index` stdout parses as
/// exactly one JSON document.
///
/// Before the fix, `crawl_page` called the printing `navigate::run` (rather
/// than the non-printing `run_core`), so every crawled page emitted its own
/// navigate envelope to stdout ahead of `index`'s own summary JSON —
/// `--max-pages 2` produced two extra documents plus the summary, breaking
/// `| jq`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_141_index_single_json_document() {
    if !live_tests_enabled() {
        eprintln!("live_141_index_single_json_document: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let Some(ff) = firefox_with_daemon("live_141_index_single_json_document") else {
        return;
    };
    let port = ff.port();

    let mut routes = HashMap::new();
    routes.insert(
        "/".to_owned(),
        FixtureRoute::html(
            "<!doctype html><title>t141 index home</title><body>\
             <a href=\"/about\">About</a> <a href=\"/contact\">Contact</a></body>",
        ),
    );
    routes.insert(
        "/about".to_owned(),
        FixtureRoute::html("<!doctype html><title>About</title><body>about page</body>"),
    );
    routes.insert(
        "/contact".to_owned(),
        FixtureRoute::html("<!doctype html><title>Contact</title><body>contact page</body>"),
    );
    let Some(site) = FixtureServer::start(routes) else {
        eprintln!("live_141_index_single_json_document: could not bind fixture HTTP — skipping");
        stop_daemon(port);
        return;
    };

    let out_dir = tempfile::tempdir().expect("temp dir");
    let map_path = out_dir.path().join("map.json");

    let out = run(
        port,
        &[
            "index",
            &site.base_url(),
            "--out",
            map_path.to_str().unwrap(),
            "--max-pages",
            "2",
            "--ignore-robots",
        ],
    );
    assert!(
        out.status.success(),
        "index failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    let doc_count = serde_json::Deserializer::from_str(stdout.trim())
        .into_iter::<Value>()
        .count();
    assert_eq!(
        doc_count, 1,
        "index stdout must parse as exactly one JSON document, got {doc_count} in:\n{stdout}"
    );

    let summary: Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("index stdout not valid JSON: {e}\n{stdout}"));
    assert!(
        summary["results"]["pages"].as_u64().unwrap_or(0) >= 1,
        "expected at least one crawled page: {summary}"
    );

    stop_daemon(port);
}

/// AC: `live_141_index_robots_user_agent_groups` — a robots.txt with a
/// foreign-UA `Disallow: /` does not block our crawl.
///
/// Reproduces the exact gov.uk shape from dogfooding session 63:
/// `User-agent: *` disallows only an unrelated `/private` path, while
/// `Disallow: /` sits under `User-agent: deepcrawl` — a named agent ff-rdp
/// does not identify as. Before the fix, the parser flattened every
/// `Disallow:` line regardless of which `User-agent:` group it belonged to,
/// applying the deepcrawl-scoped block to everyone and crawling only 1 page
/// instead of the reachable 3.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_141_index_robots_user_agent_groups() {
    if !live_tests_enabled() {
        eprintln!("live_141_index_robots_user_agent_groups: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let Some(ff) = firefox_with_daemon("live_141_index_robots_user_agent_groups") else {
        return;
    };
    let port = ff.port();

    let mut routes = HashMap::new();
    routes.insert(
        "/".to_owned(),
        FixtureRoute::html(
            "<!doctype html><title>t141 robots home</title><body>\
             <a href=\"/about\">About</a> <a href=\"/contact\">Contact</a></body>",
        ),
    );
    routes.insert(
        "/about".to_owned(),
        FixtureRoute::html("<!doctype html><title>About</title><body>about page</body>"),
    );
    routes.insert(
        "/contact".to_owned(),
        FixtureRoute::html("<!doctype html><title>Contact</title><body>contact page</body>"),
    );
    routes.insert(
        "/robots.txt".to_owned(),
        FixtureRoute {
            content_type: "text/plain",
            body: b"User-agent: *\nDisallow: /private\n\nUser-agent: deepcrawl\nDisallow: /\n"
                .to_vec(),
            extra_headers: Vec::new(),
        },
    );
    let Some(site) = FixtureServer::start(routes) else {
        eprintln!(
            "live_141_index_robots_user_agent_groups: could not bind fixture HTTP — skipping"
        );
        stop_daemon(port);
        return;
    };

    let out_dir = tempfile::tempdir().expect("temp dir");
    let map_path = out_dir.path().join("map.json");

    // Deliberately WITHOUT --ignore-robots — this is exactly what exercises
    // the parser.
    let summary = run_json(
        port,
        &[
            "index",
            &site.base_url(),
            "--out",
            map_path.to_str().unwrap(),
            "--max-pages",
            "3",
        ],
    );

    assert_eq!(
        summary["results"]["pages"], 3,
        "the deepcrawl-scoped 'Disallow: /' must not block a generic crawl \
         — expected all 3 reachable pages, got: {summary}"
    );

    stop_daemon(port);
}

// ---------------------------------------------------------------------------
// Theme C — `snapshot`: truncation visible in `meta`
// ---------------------------------------------------------------------------

/// AC: `live_141_snapshot_truncation_in_meta` — `meta` reports truncation
/// and the effective bound, rather than only a `truncated: true` marker
/// buried inside `results`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_141_snapshot_truncation_in_meta() {
    if !live_tests_enabled() {
        eprintln!("live_141_snapshot_truncation_in_meta: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let Some(ff) = firefox_with_daemon("live_141_snapshot_truncation_in_meta") else {
        return;
    };
    let port = ff.port();

    // Enough markup that a tiny --max-chars budget cannot possibly fit it
    // whole: 60 nested divs with real attributes and text.
    let mut body = String::new();
    for i in 0..60 {
        use std::fmt::Write as _;
        let _ = write!(
            body,
            "<div id=\"item-{i}\" class=\"row item-row\" data-testid=\"item-{i}\">\
             <span>Item number {i} with some descriptive text</span></div>"
        );
    }
    let html = format!("<!doctype html><title>t141 snapshot</title><body>{body}</body>");

    let mut routes = HashMap::new();
    routes.insert("/".to_owned(), FixtureRoute::html(html));
    let Some(server) = FixtureServer::start(routes) else {
        eprintln!("live_141_snapshot_truncation_in_meta: could not bind fixture HTTP — skipping");
        stop_daemon(port);
        return;
    };

    navigate(port, &server.base_url());

    let snap = run_json(port, &["snapshot", "--max-chars", "300"]);
    assert_eq!(
        snap["meta"]["max_chars"], 300,
        "meta must report the effective --max-chars bound: {snap}"
    );
    assert_eq!(
        snap["meta"]["truncated"], true,
        "meta.truncated must be true when a 300-byte budget cannot fit 60 \
         divs' worth of markup: {snap}"
    );
    assert!(
        snap["meta"].get("text_truncated").is_some(),
        "meta.text_truncated must always be present (nullable-key \
         convention), got: {snap}"
    );

    stop_daemon(port);
}

// ---------------------------------------------------------------------------
// Theme D — empty `--format text` results keep metadata
// ---------------------------------------------------------------------------

/// AC: `live_141_text_empty_result_keeps_metadata` — `a11y contrast
/// --fail-only --format text` with zero failures still reports the sampled
/// count and capped state, not a bare `[]` that reads as a clean bill of
/// health.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_141_text_empty_result_keeps_metadata() {
    if !live_tests_enabled() {
        eprintln!("live_141_text_empty_result_keeps_metadata: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let Some(ff) = firefox_with_daemon("live_141_text_empty_result_keeps_metadata") else {
        return;
    };
    let port = ff.port();

    // Plain black-on-white text — every element should pass WCAG AA, so
    // --fail-only produces zero results.
    let mut routes = HashMap::new();
    routes.insert(
        "/".to_owned(),
        FixtureRoute::html(
            "<!doctype html><title>t141 contrast</title>\
             <body style=\"background:#fff;color:#000\">\
             <h1>High contrast heading</h1>\
             <p>High contrast paragraph text.</p>\
             </body>",
        ),
    );
    let Some(server) = FixtureServer::start(routes) else {
        eprintln!(
            "live_141_text_empty_result_keeps_metadata: could not bind fixture HTTP — skipping"
        );
        stop_daemon(port);
        return;
    };

    navigate(port, &server.base_url());

    // Confirm the JSON form actually has zero failures with a nonzero
    // sample size before asserting on the text-mode rendering.
    let json = run_json(port, &["a11y", "contrast", "--fail-only"]);
    assert_eq!(json["total"], 0, "expected zero AA failures: {json}");
    let sampled = json["sampled"].as_u64().unwrap_or(0);
    assert!(
        sampled > 0,
        "expected a nonzero sample size for a page with real text: {json}"
    );

    let out = run(
        port,
        &["a11y", "contrast", "--fail-only", "--format", "text"],
    );
    assert!(
        out.status.success(),
        "a11y contrast --format text failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert_ne!(
        stdout.trim(),
        "[]",
        "empty results must not print a bare '[]' with no context: {stdout}"
    );
    assert!(
        stdout.contains(&sampled.to_string()),
        "text output must surface the sampled count ({sampled}), got:\n{stdout}"
    );

    stop_daemon(port);
}
