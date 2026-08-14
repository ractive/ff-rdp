//! Live tests for iteration 161 — "`--stringify` cannot take a script, `eval`
//! truncates long strings, and two flags fail silently".
//!
//! Themes covered:
//! - A: `--stringify` accepts exactly what bare `eval` accepts — multiple
//!   statements, ASI-separated statements from `--stdin`, and top-level
//!   `await`.
//! - B: the whole `build_script` matrix is handed to Firefox, the only JS
//!   parser this repo is allowed to use (all code stays in Rust). The old unit
//!   matrix asserted only `!contains("eval(")`, which invalid JavaScript
//!   satisfies just as well as valid JavaScript — it generated the Theme A
//!   defect on every run and passed.
//! - C: `eval` returns the whole string, fetching the `longString` grip
//!   instead of printing Firefox's ~1000-char preview as if it were the value.
//! - D: `--fields`/`--sort` reject names that appear on no result entry.
//! - E: `meta.eval_path` — a constant since iter-93 — is gone.
//!
//! daemon-parity: these use [`daemon_args`] (no `--no-daemon`) — the default
//! path every real invocation takes.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live live_161_eval_and_flag_strictness -- --nocapture

use std::collections::HashMap;
use std::io::Write as _;
use std::process::{Command, Output, Stdio};

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

/// Bring up Firefox with a running daemon, panicking on failure (iter-158
/// Theme D: an `Option` here made every caller `return`, which libtest
/// reports as `ok`).
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
    parse_json(&out, args)
}

fn parse_json(out: &Output, args: &[&str]) -> Value {
    let stdout = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("output for {args:?} not JSON: {e}\n{stdout}"))
}

fn combined(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn navigate(port: u16, url: &str) {
    let nav = run(port, &["navigate", url]);
    assert!(
        nav.status.success(),
        "navigate to {url} failed: {}",
        combined(&nav)
    );
}

/// A page with a handful of links, so `dom 'a'` has real entries for the
/// Theme D flag tests.
const LINKS_PAGE: &str = r#"<!doctype html><title>t161 links</title><body>
<a id="one" href="/a">First</a>
<a id="two" href="/b">Second</a>
<a id="three" href="/c">Third</a>
</body>"#;

/// Serve a single page at `/` and navigate to it. Returns the live server —
/// the caller must keep it alive for the duration of the test.
fn serve_and_navigate(port: u16, test: &str, html: &str) -> Option<FixtureServer> {
    let mut routes = HashMap::new();
    routes.insert("/".to_owned(), FixtureRoute::html(html.to_owned()));
    let Some(server) = FixtureServer::start(routes) else {
        eprintln!("{test}: could not bind fixture HTTP — skipping");
        return None;
    };
    navigate(port, &server.base_url());
    Some(server)
}

// ---------------------------------------------------------------------------
// Theme A — --stringify accepts what bare eval accepts
// ---------------------------------------------------------------------------

/// AC: `live_161_stringify_multi_statement_positional`.
///
/// Measured on main, 2026-08-13: `ff-rdp eval --stringify 'const x = 5; x'`
/// failed with `error: expected expression, got keyword 'const'`, while the
/// same script through bare `eval` returned `5`. The stringify wrap spliced
/// the raw text into a call-argument slot.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_161_stringify_multi_statement_positional() {
    if !live_tests_enabled() {
        eprintln!("live_161_stringify_multi_statement_positional: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_161_stringify_multi_statement_positional");
    let port = ff.port();

    let out = run(port, &["eval", "--stringify", "const x = 5; x"]);
    let text = combined(&out);
    assert!(
        out.status.success(),
        "eval --stringify 'const x = 5; x' must exit 0; got: {text}"
    );
    assert!(
        !text.contains("expected expression"),
        "the iter-161 defect is back — stringify spliced raw text: {text}"
    );
    let json = parse_json(&out, &["eval", "--stringify", "const x = 5; x"]);
    assert_eq!(json["results"], 5, "expected results == 5; got {json}");

    // A structured value survives the round-trip too — the point of the flag.
    let json = run_json(
        port,
        &["eval", "--stringify", "const o = {a:1, b:[2,3]}; o"],
    );
    assert_eq!(
        json["results"],
        serde_json::json!({"a": 1, "b": [2, 3]}),
        "expected the parsed object, not a grip; got {json}"
    );

    stop_daemon(port);
}

/// AC: `live_161_stringify_multi_statement_stdin`.
///
/// The ASI-separated form (no `;` between the last two statements at all),
/// which bare `--stdin` already returned `3` for on main while
/// `--stdin --stringify` failed to parse.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_161_stringify_multi_statement_stdin() {
    if !live_tests_enabled() {
        eprintln!("live_161_stringify_multi_statement_stdin: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_161_stringify_multi_statement_stdin");
    let port = ff.port();

    let script = "const a=1;\nconst b=2;\na+b\n";
    let mut child = Command::new(ff_rdp_bin())
        .args(daemon_args(port))
        .args(["eval", "--stdin", "--stringify"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn ff-rdp eval --stdin --stringify");
    child
        .stdin
        .take()
        .expect("stdin was piped")
        .write_all(script.as_bytes())
        .expect("write script to stdin");
    let out = child.wait_with_output().expect("wait for ff-rdp");

    let text = combined(&out);
    assert!(
        out.status.success(),
        "eval --stdin --stringify must exit 0; got: {text}"
    );
    assert!(
        !text.contains("expected expression"),
        "the iter-161 defect is back on the --stdin path: {text}"
    );
    let json = parse_json(&out, &["eval", "--stdin", "--stringify"]);
    assert_eq!(json["results"], 3, "expected results == 3; got {json}");

    stop_daemon(port);
}

/// AC: `live_161_stringify_await_multi_statement`.
///
/// The stringify wrap and the await wrap must compose with `async` on the
/// outer function — a synchronous IIFE containing `await` is a SyntaxError,
/// and an un-awaited Promise handed to the helper stringifies as `{}`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_161_stringify_await_multi_statement() {
    if !live_tests_enabled() {
        eprintln!("live_161_stringify_await_multi_statement: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_161_stringify_await_multi_statement");
    let port = ff.port();

    let script = "const r = await Promise.resolve({n:7}); r";
    let out = run(port, &["eval", "--stringify", script]);
    let text = combined(&out);
    assert!(
        out.status.success(),
        "eval --stringify with await must exit 0; got: {text}"
    );
    assert!(
        !text.contains("SyntaxError"),
        "the two wraps did not compose: {text}"
    );
    let json = parse_json(&out, &["eval", "--stringify", script]);
    assert_eq!(
        json["results"],
        serde_json::json!({"n": 7}),
        "expected the resolved value, not a pending Promise; got {json}"
    );

    stop_daemon(port);
}

// ---------------------------------------------------------------------------
// Theme B — the whole build_script matrix parses in Firefox
// ---------------------------------------------------------------------------

/// AC: `live_161_build_script_matrix_evaluates`.
///
/// The same matrix the deleted unit test
/// `build_script_never_emits_eval_for_any_combination` iterated (kept in sync
/// with `MATRIX_SCRIPTS` in `commands/eval.rs`): four scripts ×
/// `stringify ∈ {false,true}` × `isolate ∈ {false,true}`. That test asserted
/// only that the generated source contained no `eval(` — which invalid
/// JavaScript satisfies too, so it passed while generating a SyntaxError for
/// `("const x = 1; x", stringify=true)`. Firefox is the ground truth for "does
/// this parse", so the matrix runs here.
///
/// `throw new Error('boom')` legitimately fails: it surfaces its own
/// `Error: boom` through the JSON error envelope, which counts as a pass. What
/// must never appear is a `SyntaxError`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_161_build_script_matrix_evaluates() {
    if !live_tests_enabled() {
        eprintln!("live_161_build_script_matrix_evaluates: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_161_build_script_matrix_evaluates");
    let port = ff.port();

    let scripts = [
        "document.title",
        "1 + 1",
        "const x = 1; x",
        "throw new Error('boom')",
    ];
    for script in scripts {
        for stringify in [false, true] {
            for isolate in [false, true] {
                // A fresh global per combination. `const x = 1; x` on the
                // non-stringify path is sent to Firefox verbatim, so its
                // declaration persists in the page's global and the *second*
                // run of the same combination fails with "redeclaration of
                // const x" — a real property of `eval` (measured here), not
                // of the wrap under test. Reloading isolates each run so the
                // assertion below stays about parsing.
                navigate(port, "about:blank");
                let mut args = vec!["eval", script];
                if stringify {
                    args.push("--stringify");
                }
                if !isolate {
                    args.push("--no-isolate");
                }
                let out = run(port, &args);
                let text = combined(&out);
                assert!(
                    !text.contains("SyntaxError"),
                    "generated script did not parse for {script:?} \
                     (stringify={stringify}, isolate={isolate}): {text}"
                );
                assert!(
                    !text.contains("expected expression"),
                    "generated script did not parse for {script:?} \
                     (stringify={stringify}, isolate={isolate}): {text}"
                );
                if script.starts_with("throw ") {
                    // The user's own exception, not a wrapper defect.
                    assert!(
                        !out.status.success() && text.contains("boom"),
                        "the throw case must surface its own Error: boom \
                         (stringify={stringify}, isolate={isolate}): {text}"
                    );
                } else {
                    assert!(
                        out.status.success(),
                        "{script:?} must evaluate (stringify={stringify}, \
                         isolate={isolate}): {text}"
                    );
                }
            }
        }
    }

    stop_daemon(port);
}

// ---------------------------------------------------------------------------
// Theme C — eval returns the whole value
// ---------------------------------------------------------------------------

/// AC: `live_161_eval_returns_full_long_string`.
///
/// Measured on main: `ff-rdp eval '"x".repeat(5000)' --jq '.results | length'`
/// printed ~1000 — Firefox's inline preview — with no `meta.truncated`, no
/// hint, and the `longString` actor released a few lines later, so the rest
/// was unreachable by any command.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_161_eval_returns_full_long_string() {
    if !live_tests_enabled() {
        eprintln!("live_161_eval_returns_full_long_string: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_161_eval_returns_full_long_string");
    let port = ff.port();

    let out = run(port, &["eval", "\"x\".repeat(5000)"]);
    let text = combined(&out);
    assert!(out.status.success(), "eval must exit 0; got: {text}");
    let json = parse_json(&out, &["eval", "\"x\".repeat(5000)"]);
    let s = json["results"]
        .as_str()
        .unwrap_or_else(|| panic!("results must be a string, not a grip; got {json}"));
    assert_eq!(s.len(), 5000, "expected all 5000 chars, got {}", s.len());
    assert!(
        !out.stdout
            .windows(20)
            .any(|w| w == b"\"type\":\"longString\""),
        "no longString grip may appear anywhere in the envelope: {text}"
    );

    stop_daemon(port);
}

/// AC: `live_161_eval_stringify_long_payload_parses`.
///
/// A stringified payload well over the ~1000-char inline limit comes back as a
/// `longString` too. Before iter-161 the JSON parse in `eval::run` saw the
/// grip's `to_json()` object rather than a `Value::String`, so it silently
/// skipped — the caller got a grip and not even a `meta.stringify_parsed:
/// false` to tell them.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_161_eval_stringify_long_payload_parses() {
    if !live_tests_enabled() {
        eprintln!("live_161_eval_stringify_long_payload_parses: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_161_eval_stringify_long_payload_parses");
    let port = ff.port();

    let script = "Array.from({length:400},(_,i)=>({i}))";
    let json = run_json(port, &["eval", "--stringify", script]);
    let arr = json["results"]
        .as_array()
        .unwrap_or_else(|| panic!("results must be a parsed array; got {json}"));
    assert_eq!(arr.len(), 400, "expected 400 elements, got {}", arr.len());
    assert_eq!(arr[399], serde_json::json!({"i": 399}));
    assert!(
        json["meta"].get("stringify_parsed").is_none(),
        "the parse must succeed, so no stringify_parsed flag; got {json}"
    );

    stop_daemon(port);
}

// ---------------------------------------------------------------------------
// Theme D — --fields / --sort fail loud
// ---------------------------------------------------------------------------

/// AC: `live_161_fields_and_sort_reject_unknown_names`.
///
/// Measured on main: `--fields bogusfield` printed
/// `{"results": [{}, {}], "total": 2}` at exit 0 (the data destroyed) and
/// `--sort nosuchfield` was a silent no-op.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_161_fields_and_sort_reject_unknown_names() {
    if !live_tests_enabled() {
        eprintln!("live_161_fields_and_sort_reject_unknown_names: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_161_fields_and_sort_reject_unknown_names");
    let port = ff.port();
    let Some(_server) = serve_and_navigate(
        port,
        "live_161_fields_and_sort_reject_unknown_names",
        LINKS_PAGE,
    ) else {
        stop_daemon(port);
        return;
    };

    for (flag, name) in [("--fields", "bogusfield"), ("--sort", "nosuchfield")] {
        let out = run(port, &["dom", "a", "--limit", "2", flag, name]);
        let text = combined(&out);
        assert_eq!(
            out.status.code(),
            Some(1),
            "{flag} {name} must exit 1; got: {text}"
        );
        let json = parse_json(&out, &["dom", "a", "--limit", "2", flag, name]);
        let msg = json["error"]
            .as_str()
            .unwrap_or_else(|| panic!("expected a JSON error envelope; got {json}"));
        assert_eq!(json["error_type"], "User", "got {json}");
        assert!(msg.contains(flag), "message must name the flag: {msg}");
        assert!(msg.contains(name), "message must name the offender: {msg}");
        assert!(
            msg.contains("tag"),
            "message must list at least one available key: {msg}"
        );
    }

    // The working form is unchanged.
    //
    // The plan's AC named `--fields tag,text` as the control case, but `dom`
    // emits no `text` key at all — its entries are `attrs`, `name`, `ref`,
    // `tag` (the accessible name, not the text content; `--text-attrs` adds
    // `textContent`). On main `--fields tag,text` therefore returned only
    // `tag` and silently dropped `text`, which is a milder instance of the
    // very defect Theme D fixes; under DEC-035 it is now an error. `tag,name`
    // is the equivalent two-key control that actually exercises the pass path.
    let json = run_json(port, &["dom", "a", "--limit", "2", "--fields", "tag,name"]);
    let arr = json["results"]
        .as_array()
        .unwrap_or_else(|| panic!("expected an array; got {json}"));
    assert_eq!(arr.len(), 2, "expected 2 entries; got {json}");
    for entry in arr {
        assert!(entry.get("tag").is_some(), "tag must survive: {entry}");
        assert!(entry.get("name").is_some(), "name must survive: {entry}");
        assert!(
            entry.get("attrs").is_none(),
            "attrs must be filtered: {entry}"
        );
    }

    // And the plan's original control case, recorded as measured: `text` is
    // not a key `dom` ever emits, so it is rejected like any other typo.
    let out = run(port, &["dom", "a", "--limit", "2", "--fields", "tag,text"]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "`text` is not a dom key — it must be rejected: {}",
        combined(&out)
    );

    let out = run(
        port,
        &["dom", "a", "--limit", "2", "--sort", "tag", "--asc"],
    );
    assert!(
        out.status.success(),
        "--sort tag --asc must still work: {}",
        combined(&out)
    );

    // An empty result set is not an error: there is no union to validate
    // against, so the field name cannot be judged.
    let out = run(port, &["dom", ".no-such-class-anywhere", "--fields", "tag"]);
    let text = combined(&out);
    assert!(
        out.status.success(),
        "an empty result set must stay exit 0: {text}"
    );
    let json = parse_json(&out, &["dom", ".no-such-class-anywhere", "--fields", "tag"]);
    assert_eq!(json["total"], 0, "expected total 0; got {json}");

    stop_daemon(port);
}

// ---------------------------------------------------------------------------
// Theme E — meta.eval_path is gone
// ---------------------------------------------------------------------------

/// AC: `live_161_eval_meta_has_no_eval_path`.
///
/// The field was hard-set to `"page-await"` on every call since iter-93
/// deleted its only other value; DEC-020 confirmed that deletion stands.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_161_eval_meta_has_no_eval_path() {
    if !live_tests_enabled() {
        eprintln!("live_161_eval_meta_has_no_eval_path: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_161_eval_meta_has_no_eval_path");
    let port = ff.port();

    let json = run_json(port, &["eval", "document.title"]);
    let meta = json["meta"]
        .as_object()
        .unwrap_or_else(|| panic!("meta must be an object; got {json}"));
    assert!(
        !meta.contains_key("eval_path"),
        "meta.eval_path was removed in iter-161; got {json}"
    );

    stop_daemon(port);
}
