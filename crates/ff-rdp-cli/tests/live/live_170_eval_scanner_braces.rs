//! Live tests for iteration 170 — "`eval`'s scanner still cannot see into
//! `${}` or past a `}`".
//!
//! iter-167 taught `top_level_statement_boundaries` about regex literals,
//! comments and backslash escapes, and documented two gaps it left open: a
//! `${…}` interpolation was skipped as opaque template text, and a `/` right
//! after `}` was always read as division. It asserted both fail *safe* — "the
//! worst outcome is a boundary the scanner should not have reported, which
//! costs at most a wrap".
//!
//! Measured on main 2026-08-17 against a live Firefox (the Theme A table in
//! `kb/iterations/iteration-170-*`), neither did:
//!
//! - ``eval --stringify 'const s = `a${"`"}b`; s'``
//!   → `{"results":{"type":"undefined"}}` instead of ``a`b``. The interpolated
//!   backtick closed the template, the `"` after it opened double-quote state,
//!   and the script's real top-level `;` was swallowed as string content — so
//!   no boundary was reported, the iter-165 wrap had nothing to auto-return,
//!   and the value was silently lost. iter-142 Theme E named exactly this the
//!   worst failure mode of this wrap.
//! - `eval --stringify 'const n = 1; if (n) {} /a;b/.test("a;b")'`
//!   → `{"error":"unterminated regular expression literal"}`. The `/` was
//!   scanned as a division, so the `;` *inside* the regex was reported as a
//!   top-level boundary and the wrap split the script into `… {} /a;` and
//!   `b/.test("a;b")`.
//!
//! Firefox is the only JS parser this repo may use as ground truth (all code
//! stays in Rust and there is no in-process JS parser), so "does the generated
//! script parse, and does it still produce the right value" is asserted here.
//! The unit tests in `commands/eval.rs` (`unit_170_*`) pin the boundary sets
//! that produce it. See DEC-042.
//!
//! daemon-parity: these use the default daemon path, like every real
//! invocation.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live live_170_eval_scanner_braces -- --nocapture

use std::process::{Command, Output};

use serde_json::Value;

use crate::common::{LiveFirefox, ff_rdp_bin, live_tests_enabled};

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

fn combined(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// Run `ff-rdp eval <flags> <script>` and return `results`, asserting exit 0
/// and that nothing in the output looks like a wrapper-generated parse error.
fn eval_ok(port: u16, script: &str, flags: &[&str]) -> Value {
    let mut args = vec!["eval"];
    args.extend_from_slice(flags);
    args.push(script);
    let out = run(port, &args);
    let text = combined(&out);
    for marker in [
        "unterminated regular expression",
        "unescaped line break",
        "expected expression",
        "SyntaxError",
    ] {
        assert!(
            !text.contains(marker),
            "the wrap emitted invalid JavaScript for {script:?} {flags:?} \
             ({marker}): {text}"
        );
    }
    assert!(
        out.status.success(),
        "eval {flags:?} {script:?} must exit 0; got: {text}"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let parsed: Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("output for {args:?} not JSON: {e}\n{stdout}"));
    parsed["results"].clone()
}

// ---------------------------------------------------------------------------
// AC live_170_interpolation_and_brace_scripts_evaluate
// ---------------------------------------------------------------------------

/// AC: `live_170_interpolation_is_scanned` (live half) — gap 1.
///
/// Every value below is the one the script produces when run by hand in a JS
/// console, so a wrap that silently changed the value — which is precisely how
/// this defect presented, as `{"type":"undefined"}` rather than a parse error —
/// is caught, not just one that fails to parse.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_170_interpolation_is_scanned_as_code() {
    if !live_tests_enabled() {
        eprintln!("live_170_interpolation_is_scanned_as_code: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_170_interpolation_is_scanned_as_code");
    let port = ff.port();

    let cases: &[(&str, Value)] = &[
        // The plan's headline case: on main this returned `undefined`.
        (r#"const s = `a${"`"}b`; s"#, Value::from("a`b")),
        (r"const s = `a${'`'}b`; s", Value::from("a`b")),
        // A `;` and a quote inside an interpolation.
        (r#"const s = `x${ ";" }y`; s"#, Value::from("x;y")),
        (
            r#"const t = `v=${JSON.stringify({a:";"})}`; t"#,
            Value::from(r#"v={"a":";"}"#),
        ),
        // Code inside the interpolation is now scanned as code: a regex, a
        // comment, a nested template.
        (
            r#"const m = `m=${/a;b/.test("a;b")}`; m"#,
            Value::from("m=true"),
        ),
        ("const m = `m=${ 1 /* a; b */ + 2 }`; m", Value::from("m=3")),
        ("const m = `a${ `n${1}m` }b`; m", Value::from("an1mb")),
    ];

    for (script, expected) in cases {
        for flags in [&[][..], &["--stringify"][..]] {
            assert_eq!(
                &eval_ok(port, script, flags),
                expected,
                "{script:?} with {flags:?} must evaluate to {expected}"
            );
        }
    }

    stop_daemon(port);
}

/// Gap 2 and its guard rail: a `/` after a *block*'s `}` opens a regex, a `/`
/// after an *object literal*'s `}` divides, and a top-level block ends its own
/// statement so the wrap can auto-return what follows it.
///
/// The second half of the table is the direction that must NOT regress —
/// reading a division as a regex swallows text, which is the one failure mode
/// worse than the two this iteration fixed.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_170_brace_kind_decides_regex_and_boundary() {
    if !live_tests_enabled() {
        eprintln!("live_170_brace_kind_decides_regex_and_boundary: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_170_brace_kind_decides_regex_and_boundary");
    let port = ff.port();

    let cases: &[(&str, Value)] = &[
        // The plan's gap-2 case: `unterminated regular expression literal` on
        // main, both with and without a newline before the regex.
        (
            r#"const n = 1; if (n) {} /a;b/.test("a;b")"#,
            Value::Bool(true),
        ),
        (
            "const n = 1; if (n) {}\n/a;b/.test(\"a;b\")",
            Value::Bool(true),
        ),
        // A block terminates its own statement, so the trailing expression is
        // auto-returned with no `;` and no newline after the `}`.
        ("for (const a of [1,2]) {} 7", Value::from(7)),
        ("function f(){ return 5 } f()", Value::from(5)),
        ("let a = 1; function f(){ return a+1 } f()", Value::from(2)),
        ("switch (1) { case 1: break; } 11", Value::from(11)),
        // Division after an object literal must stay division.
        ("const o = {v: 8}; o.v / 2", Value::from(4)),
        ("const o = {v: 8}; o.v / 2; o.v / 4", Value::from(2)),
        // Clause keywords continue the construct — no boundary, no change.
        ("const n = 1; if (n) { 2 } else { 3 }", Value::Null),
        ("const q = 1; try { q } catch (e) { 2 }", Value::Null),
        // The IIFE form: `(` after `}` applies to what precedes it.
        ("const r = !function(){ return 1 }(); r", Value::Bool(false)),
        // A block-scoped declaration must not become a top-level one
        // (`eval --help`: this shape skips the wrap entirely).
        ("if (1) { const z = 2 }", Value::Null),
    ];

    for (script, expected) in cases {
        for flags in [&[][..], &["--stringify"][..]] {
            let got = eval_ok(port, script, flags);
            // `undefined` comes back as an actor grip, not JSON null.
            let got_is_undefined = got["type"] == "undefined";
            if expected.is_null() {
                assert!(
                    got_is_undefined || got.is_null(),
                    "{script:?} with {flags:?} must stay undefined (the wrap has \
                     nothing safe to auto-return), got {got}"
                );
            } else {
                assert_eq!(
                    &got, expected,
                    "{script:?} with {flags:?} must evaluate to {expected}"
                );
            }
        }
    }

    stop_daemon(port);
}
