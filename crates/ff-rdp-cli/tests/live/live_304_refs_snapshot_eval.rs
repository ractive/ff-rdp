//! Live tests for the v0.4.0 pre-release fix round (dogfooding session 64
//! #3, #4, #5, #21, #27, #31): `snapshot` hands out refs past the depth cut
//! and bounds its printed bytes, `--query` searches the whole document, a
//! stale `--ref` fails fast as `stale_ref`, and `eval` reports a rejected
//! Promise as an error.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live live_304 -- --include-ignored

use std::collections::HashMap;
use std::fmt::Write as _;
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::common::{FixtureRoute, FixtureServer, LiveFirefox, ff_rdp_bin, live_tests_enabled};

fn cli_args(port: u16) -> Vec<String> {
    vec![
        "--host".to_owned(),
        "127.0.0.1".to_owned(),
        "--port".to_owned(),
        port.to_string(),
        "--timeout".to_owned(),
        "20000".to_owned(),
        "--no-hints".to_owned(),
    ]
}

fn run(port: u16, args: &[&str]) -> Output {
    Command::new(ff_rdp_bin())
        .args(cli_args(port))
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("spawn ff-rdp {args:?}: {e}"))
}

fn run_json(port: u16, args: &[&str]) -> Value {
    let out = run(port, args);
    assert!(
        out.status.success(),
        "command {args:?} failed: {}",
        crate::common::output_note(&out)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("output for {args:?} not JSON: {e}\n{stdout}"))
}

/// The JSON error envelope a failing command prints on stdout.
fn error_envelope(out: &Output) -> Value {
    let stdout = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
        panic!(
            "error output not JSON: {e}\n{}",
            crate::common::output_note(out)
        )
    })
}

/// Every `ref` anywhere in a snapshot tree, including `refs_below` entries.
fn collect_refs(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Object(map) => {
            if let Some(Value::String(r)) = map.get("ref") {
                out.push(r.clone());
            }
            for child in map.values() {
                collect_refs(child, out);
            }
        }
        Value::Array(items) => items.iter().for_each(|i| collect_refs(i, out)),
        _ => {}
    }
}

/// A page whose links all sit 12 levels deep — below the default depth of 6,
/// the shape that gave HN and react.dev 0 refs — with a needle at the bottom.
fn deep_page() -> String {
    let mut html = String::from("<!doctype html><title>t304 deep</title><body>");
    for _ in 0..12 {
        html.push_str("<div>");
    }
    for i in 0..40 {
        let _ = write!(html, "<a href=\"/item{i}\">item number {i}</a> ");
    }
    html.push_str("<p>needle-token-304</p>");
    for _ in 0..12 {
        html.push_str("</div>");
    }
    html.push_str("</body>");
    html
}

/// A page large enough that its printed snapshot is far over 20 KB.
fn wide_page() -> String {
    let mut html = String::from("<!doctype html><title>t304 wide</title><body>");
    for i in 0..400 {
        let _ = write!(
            html,
            "<div class=\"row-{i}\"><span><b><i>cell {i} with some filler text</i></b></span></div>"
        );
    }
    html.push_str("</body>");
    html
}

fn serve(routes: &[(&str, String)]) -> Option<FixtureServer> {
    let map: HashMap<String, FixtureRoute> = routes
        .iter()
        .map(|(path, body)| ((*path).to_owned(), FixtureRoute::html(body)))
        .collect();
    FixtureServer::start(map)
}

/// Default-depth `snapshot` gives a ref to every link below the depth cut and
/// says the tree was truncated, with a hint naming `--depth` / `--query`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_304_snapshot_refs_past_depth_cut() {
    if !live_tests_enabled() {
        eprintln!("live_304_snapshot_refs_past_depth_cut: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();
    let Some(server) = serve(&[("/", deep_page())]) else {
        eprintln!("live_304: could not bind fixture HTTP — skipping");
        return;
    };
    run_json(port, &["navigate", &server.base_url()]);

    let snap = run_json(port, &["snapshot"]);
    let mut refs = Vec::new();
    collect_refs(&snap["results"], &mut refs);
    assert!(
        refs.len() >= 40,
        "every deep link needs a ref, got {}: {snap}",
        refs.len()
    );
    assert_eq!(snap["meta"]["truncated"], true, "{}", snap["meta"]);
    assert_eq!(snap["meta"]["depth_truncated"], true, "{}", snap["meta"]);
    let hint = snap["meta"]["hint"].as_str().unwrap_or_default();
    assert!(
        hint.contains("--depth") && hint.contains("--query"),
        "{hint}"
    );

    // A ref from `refs_below` is a working handle.
    let clicked = run_json(port, &["click", "--ref", &refs[1]]);
    assert!(clicked["results"].is_object(), "{clicked}");
}

/// `--query` finds a match below the default depth.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_304_snapshot_query_searches_whole_document() {
    if !live_tests_enabled() {
        eprintln!("live_304_snapshot_query_searches_whole_document: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();
    let Some(server) = serve(&[("/", deep_page())]) else {
        eprintln!("live_304: could not bind fixture HTTP — skipping");
        return;
    };
    run_json(port, &["navigate", &server.base_url()]);

    let snap = run_json(port, &["snapshot", "--query", "needle-token-304"]);
    assert_eq!(snap["meta"]["matches"], 1, "{snap}");
}

/// `--max-chars` bounds the bytes actually printed, not compact JSON.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_304_snapshot_max_chars_bounds_printed_bytes() {
    if !live_tests_enabled() {
        eprintln!("live_304_snapshot_max_chars_bounds_printed_bytes: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();
    let Some(server) = serve(&[("/", wide_page())]) else {
        eprintln!("live_304: could not bind fixture HTTP — skipping");
        return;
    };
    run_json(port, &["navigate", &server.base_url()]);

    let out = run(port, &["snapshot", "--depth", "20", "--max-chars", "20000"]);
    assert!(out.status.success(), "{}", crate::common::output_note(&out));
    let printed = out.stdout.len();
    // 20000 for the tree plus the envelope's `meta`.
    assert!(
        printed < 22_000,
        "printed {printed} bytes for --max-chars 20000"
    );
    let snap: Value = serde_json::from_slice(&out.stdout).expect("snapshot JSON");
    assert_eq!(snap["meta"]["truncated"], true, "{}", snap["meta"]);
}

/// A ref taken before a navigation fails as `stale_ref` within a second
/// instead of timing out or clicking an element of the new page.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_304_stale_ref_fails_fast_after_navigation() {
    if !live_tests_enabled() {
        eprintln!("live_304_stale_ref_fails_fast_after_navigation: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();
    let page = |label: &str| {
        format!(
            "<!doctype html><title>{label}</title><body>\
             <button onclick=\"document.title='clicked-{label}'\">first {label}</button>\
             <button>second {label}</button></body>"
        )
    };
    let Some(server) = serve(&[("/a", page("a")), ("/b", page("b"))]) else {
        eprintln!("live_304: could not bind fixture HTTP — skipping");
        return;
    };
    let base = server.base_url();
    run_json(port, &["navigate", &format!("{base}/a")]);
    let mut old_refs = Vec::new();
    collect_refs(&run_json(port, &["snapshot"])["results"], &mut old_refs);
    let stale = old_refs.first().expect("page a has refs").clone();

    run_json(port, &["navigate", &format!("{base}/b")]);
    // Stamp page b too, so a ref numbering that restarted per document would
    // hand `stale` to one of b's buttons.
    let mut new_refs = Vec::new();
    collect_refs(&run_json(port, &["snapshot"])["results"], &mut new_refs);
    assert!(
        !new_refs.contains(&stale),
        "refs must differ per document: {stale} in {new_refs:?}"
    );

    let started = Instant::now();
    let out = run(port, &["click", "--ref", &stale]);
    let elapsed = started.elapsed();
    assert!(
        !out.status.success(),
        "a stale ref must not click: {}",
        crate::common::output_note(&out)
    );
    let err = error_envelope(&out);
    assert_eq!(err["error_type"], "stale_ref", "{err}");
    assert_eq!(err["ref"], stale.as_str(), "{err}");
    assert!(
        err["error"]
            .as_str()
            .unwrap_or_default()
            .contains("run snapshot again"),
        "{err}"
    );
    assert!(
        elapsed < Duration::from_secs(1),
        "stale ref took {elapsed:?}"
    );

    let title = run_json(port, &["eval", "document.title"]);
    assert_eq!(
        title["results"], "b",
        "nothing on page b may be clicked: {title}"
    );

    // A command that does not auto-wait reports the same error type.
    let styles = run(port, &["styles", "--ref", &stale]);
    assert_eq!(error_envelope(&styles)["error_type"], "stale_ref");
}

/// A rejected Promise — returned or awaited — gets the error envelope a
/// synchronous throw gets, with exit code 1.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_304_eval_rejected_promise_reports_error() {
    if !live_tests_enabled() {
        eprintln!("live_304_eval_rejected_promise_reports_error: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = LiveFirefox::headless_on_random_port();
    let port = ff.port();

    let sync = run(port, &["eval", "throw new Error(\"boom\")"]);
    assert_eq!(
        sync.status.code(),
        Some(1),
        "{}",
        crate::common::output_note(&sync)
    );
    let sync_err = error_envelope(&sync);

    for script in [
        "Promise.reject(new Error(\"boom\"))",
        "await Promise.reject(new Error(\"boom\"))",
    ] {
        let out = run(port, &["eval", script]);
        assert_eq!(
            out.status.code(),
            Some(1),
            "{script}: {}",
            crate::common::output_note(&out)
        );
        let err = error_envelope(&out);
        assert_eq!(err["error"], "boom", "{script}: {err}");
        assert_eq!(err["error_type"], sync_err["error_type"], "{script}: {err}");
        assert_eq!(err["promise_rejected"], true, "{script}: {err}");
        assert!(err["stack"].is_string(), "{script}: {err}");
    }

    // A failing fetch (closed local port, no network needed).
    let fetch = run(port, &["eval", "fetch(\"http://127.0.0.1:1/\")"]);
    assert_eq!(
        fetch.status.code(),
        Some(1),
        "{}",
        crate::common::output_note(&fetch)
    );
    let err = error_envelope(&fetch);
    assert_eq!(err["error_type"], "User", "{err}");
    assert!(
        err["error"]
            .as_str()
            .unwrap_or_default()
            .contains("NetworkError"),
        "{err}"
    );

    // A resolved Promise is unaffected.
    let ok = run_json(port, &["eval", "Promise.resolve(42)"]);
    assert_eq!(ok["results"], 42, "{ok}");
}
