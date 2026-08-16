//! Live tests for iteration 167 — "`eval`'s statement scanner mis-splits regex
//! literals and comments".
//!
//! `top_level_statement_boundaries` decides where one top-level JS statement
//! ends and the next begins, and all three `eval` wraps are built on it: the
//! `--stringify` wrap (iter-161), the top-level-`await` wrap (iter-132/142)
//! and the iter-165 per-call scope wrap. It was a character scanner that
//! tracked quotes and bracket depth only, so a `;` inside a regex literal or a
//! comment looked like a statement separator and the wrap emitted invalid
//! JavaScript.
//!
//! Measured on main 2026-08-16 against a live Firefox, before the fix (the
//! Theme A table in `kb/iterations/iteration-167-*`):
//!
//! - `eval --stringify '/a;b/.test("a;b")'`
//!   → `{"error":"unterminated regular expression literal"}`
//! - `eval --stringify 'const s = "x"; /a;b/.test("a;b")'` → same
//! - `eval --stringify 'const s = "a\";b"; s'`
//!   → `{"error":"\"\" string literal contains an unescaped line break"}`
//! - `eval --stringify 'const s = ` + "`a\\`;b`" + `; s'`
//!   → `{"error":"expected expression, got ')'"}`
//! - `eval --stringify "// don't touch\nconst x = 1; x"`
//!   → `{"error":"expected expression, got keyword 'const'"}`
//!
//! Firefox is the only JS parser this repo may use as ground truth (all code
//! stays in Rust and there is no in-process JS parser), so "does the generated
//! script parse" is asserted here rather than in a unit test. The unit tests in
//! `commands/eval.rs` (`unit_167_*`) pin the boundary sets that produce it.
//!
//! daemon-parity: these use the default daemon path, like every real
//! invocation.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live live_167_eval_scanner_tokens -- --nocapture

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

fn parse_json(out: &Output, args: &[&str]) -> Value {
    let stdout = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("output for {args:?} not JSON: {e}\n{stdout}"))
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
    parse_json(&out, &args)["results"].clone()
}

// ---------------------------------------------------------------------------
// AC live_167_regex_literal_survives_every_wrap
// ---------------------------------------------------------------------------

/// AC: `live_167_regex_literal_survives_every_wrap`.
///
/// The plan's headline case, through all three wrap paths. `/a;b/.test("a;b")`
/// contains a top-level `;` inside a regex literal:
///
/// - **plain** — declaration-free, so iter-165 leaves it unwrapped; this has
///   always worked and must keep working.
/// - **`--stringify`** — the wrap the defect was measured on. On main this
///   returned `unterminated regular expression literal`.
/// - **`await`** — the async-IIFE wrap, reached by putting the same regex call
///   inside `await Promise.resolve(…)`, plus the variant where the regex sits
///   at top level *next to* an `await` so it is not shielded by bracket depth.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_167_regex_literal_survives_every_wrap() {
    if !live_tests_enabled() {
        eprintln!("live_167_regex_literal_survives_every_wrap: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_167_regex_literal_survives_every_wrap");
    let port = ff.port();

    let regex_call = r#"/a;b/.test("a;b")"#;

    assert_eq!(
        eval_ok(port, regex_call, &[]),
        Value::Bool(true),
        "the plain path must evaluate the regex call"
    );
    assert_eq!(
        eval_ok(port, regex_call, &["--stringify"]),
        Value::Bool(true),
        "--stringify must not split the script inside the regex literal"
    );
    assert_eq!(
        eval_ok(
            port,
            &format!("await Promise.resolve({regex_call})"),
            &["--stringify"],
        ),
        Value::Bool(true),
        "the await wrap composed with --stringify must not split the regex"
    );

    // The regex at top level in a multi-statement script — the shape that
    // actually reaches `wrap_statements_in_iife` on each path.
    let declaring = format!(r#"const s = "x"; {regex_call}"#);
    for flags in [&[][..], &["--stringify"][..]] {
        assert_eq!(
            eval_ok(port, &declaring, flags),
            Value::Bool(true),
            "a regex as the last statement must survive {flags:?}"
        );
    }
    assert_eq!(
        eval_ok(
            port,
            &format!("const s = await Promise.resolve(\"x\"); {regex_call}"),
            &[],
        ),
        Value::Bool(true),
        "a regex as the last statement of an await script must survive"
    );

    stop_daemon(port);
}

// ---------------------------------------------------------------------------
// Theme B — comments and backslash escapes
// ---------------------------------------------------------------------------

/// The rest of the Theme A measurement table: every input that produced
/// invalid JavaScript on main, plus the division cases that must NOT be
/// mistaken for regex literals now that the scanner knows what one is.
///
/// Each `results` value is the one the script would produce if it were run by
/// hand in a JS console, so a wrap that silently changed the value (rather
/// than failing to parse) is caught too.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_167_comments_and_escapes_do_not_split_scripts() {
    if !live_tests_enabled() {
        eprintln!("live_167_comments_and_escapes_do_not_split_scripts: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_167_comments_and_escapes_do_not_split_scripts");
    let port = ff.port();

    let cases: &[(&str, Value)] = &[
        // #10 on main: `"" string literal contains an unescaped line break`.
        (r#"const s = "a\";b"; s"#, Value::from("a\";b")),
        // #16 on main: `expected expression, got ')'`.
        ("const s = `a\\`;b`; s", Value::from("a`;b")),
        // #13 on main: `expected expression, got keyword 'const'` — the
        // apostrophe opened string state and swallowed the declaration.
        ("// don't touch\nconst x = 1; x", Value::from(1)),
        ("/* don't touch */ const y = 2; y", Value::from(2)),
        ("const t = 1; // a; b\nt", Value::from(1)),
        ("const u = 1 /* a; b */; u", Value::from(1)),
        // Division must still be division, and the `;` between the two halves
        // must still be a statement boundary.
        ("const a = 8; a / 2", Value::from(4)),
        ("const b = (4 + 4) / 2; b", Value::from(4)),
        // A regex after a keyword, and one nested in an argument list.
        (
            r#"const f = () => { return /a;b/.test("a;b") }; f()"#,
            Value::Bool(true),
        ),
        (
            "const xs = ['a;b']; xs.filter(v => /a;b/.test(v)).length",
            Value::from(1),
        ),
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

/// iter-165's per-call scope must reach a declaration that sits behind a
/// comment. On main the leading `//` opened string state, so
/// `declares_at_top_level` never saw the `const` and the script stayed on the
/// unwrapped path — the binding leaked into the tab's global lexical
/// environment and a second identical call died with `redeclaration of const`.
///
/// This is the iter-165 contract (DEC-039), re-asserted for the input class
/// iter-167 unblocked, so it runs the same script three times.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_167_commented_declaration_is_still_isolated() {
    if !live_tests_enabled() {
        eprintln!("live_167_commented_declaration_is_still_isolated: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_167_commented_declaration_is_still_isolated");
    let port = ff.port();

    for attempt in 1..=3 {
        assert_eq!(
            eval_ok(port, "// set it up\nconst scoped = 7; scoped", &[]),
            Value::from(7),
            "attempt {attempt}: a commented declaration must stay call-local"
        );
    }

    stop_daemon(port);
}
