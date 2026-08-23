//! Live tests for iteration 176 — "the `eval` scanner refuses to judge three
//! brace positions".
//!
//! iter-170 gave `top_level_statement_boundaries` a `BraceKind` stack but had
//! `brace_opens_block` commit only where a statement can start *and* an object
//! literal cannot. That left three positions in the conservative
//! `ObjectLiteral` bucket: an arrow function's `{` body, a `class` body, and a
//! labelled block. iter-170 assumed all three failed safe.
//!
//! Measured on `main` 2026-08-23 against a live Firefox (the Theme A table in
//! `kb/iterations/iteration-176-eval-scanner-brace-positions.md`), none did.
//! Firefox's own answer for each source was obtained by handing the raw source
//! to an indirect `eval`, so the wrap could not influence the ground truth:
//!
//! - `eval --stringify 'class K { m(){ return 9 } } new K().m()'` →
//!   `{"results":{"type":"undefined"}}` where Firefox returns `9`. A silent
//!   wrong *value*, not an error — iter-142 Theme E named exactly this the
//!   worst failure mode of this wrap. No boundary followed the class body's
//!   `}`, so `new K().m()` was never the last statement.
//! - `eval --stringify 'const n = 1; outer: { break outer } n'` →
//!   `{"error":"missing ) in parenthetical"}` where Firefox returns `1`.
//! - With a real line terminator — i.e. real ASI, which is what makes the
//!   source valid JavaScript in the first place — all three positions reach
//!   the same defect: `const g = () => {}\n/a;b/.test("a;b")`,
//!   `class K { m(){ return 9 } }\n/a;b/.test("a;b")` and
//!   `const n = 1; outer: { break outer }\n/a;b/.test("a;b")` each threw
//!   `unterminated regular expression literal` where Firefox returns `true`.
//!   The `}` read as an object literal's, so the `/` read as a division and
//!   the `;` *inside* the regex became a top-level boundary.
//!
//! Firefox is the only JS parser this repo may use as ground truth (all code
//! stays in Rust and there is no in-process JS parser), so "does the generated
//! script parse, and does it still produce the right value" is asserted here.
//! The unit tests in `commands/eval.rs` (`unit_176_*`) pin the boundary sets
//! that produce it. See DEC-042.
//!
//! daemon-parity: these use the default daemon path, like every real
//! invocation.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live live_176_eval_scanner_brace_positions -- --nocapture

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
        "missing ) in parenthetical",
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

/// Assert `script` evaluates to `expected`, where `Value::Null` stands for
/// "undefined", which comes back as an actor grip rather than JSON null.
fn assert_evaluates(port: u16, script: &str, expected: &Value) {
    for flags in [&[][..], &["--stringify"][..]] {
        let got = eval_ok(port, script, flags);
        if expected.is_null() {
            assert!(
                got["type"] == "undefined" || got.is_null(),
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

// ---------------------------------------------------------------------------
// AC 1 — each of the three positions is fixed with a live test
// ---------------------------------------------------------------------------

/// AC 1: the three positions iter-170 left unjudged now evaluate to the value
/// Firefox itself produces for the same source.
///
/// Every expectation here is Firefox's own answer, taken from the Theme A
/// measurement table, so a wrap that silently changed the *value* — which is
/// how the class-declaration case presented, as `undefined` rather than an
/// error — is caught, not just one that fails to parse.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_176_arrow_class_and_label_bodies_are_blocks() {
    if !live_tests_enabled() {
        eprintln!("live_176_arrow_class_and_label_bodies_are_blocks: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_176_arrow_class_and_label_bodies_are_blocks");
    let port = ff.port();

    let cases: &[(&str, Value)] = &[
        // Position 1 — an arrow function's block body. `unterminated regular
        // expression literal` on main.
        ("const g = () => {}\n/a;b/.test(\"a;b\")", Value::Bool(true)),
        (
            "const g = async () => {}\n/a;b/.test(\"a;b\")",
            Value::Bool(true),
        ),
        // Position 2 — a class body. The silent-wrong-value case first, then
        // the ASI form.
        ("class K { m(){ return 9 } } new K().m()", Value::from(9)),
        (
            "class K { m(){ return 9 } }\n/a;b/.test(\"a;b\")",
            Value::Bool(true),
        ),
        ("class K extends Object {} K.name", Value::from("K")),
        // Review fix: a namespaced superclass (`extends Ns.Base`, the shape
        // `extends React.Component` and `extends stream.Writable` take) hit
        // the same silent-`undefined` defect as the bare-identifier case
        // above, because the dotted-property guard that (correctly) excludes
        // `obj.try {` fired before the class/`extends` lookback ever ran.
        // Fixed alongside the bare-identifier case; pinned here since the
        // first landing of this position's tests only ever covered
        // `extends Object`.
        (
            "const NS = {Base: class { m(){ return 9 } }}; \
             class K extends NS.Base {} new K().m()",
            Value::from(9),
        ),
        // Position 3 — a labelled block. `missing ) in parenthetical` on main.
        ("const n = 1; outer: { break outer } n", Value::from(1)),
        (
            "const n = 1; outer: { break outer }\n/a;b/.test(\"a;b\")",
            Value::Bool(true),
        ),
        ("outer: { break outer } 5", Value::from(5)),
    ];

    for (script, expected) in cases {
        assert_evaluates(port, script, expected);
    }

    stop_daemon(port);
}

// ---------------------------------------------------------------------------
// AC 2 — the divisions the conservative bucket protected still divide
// ---------------------------------------------------------------------------

/// AC 2: the A/B guard rail. Reading a division as a regex swallows text,
/// which is the one failure mode worse than the three this iteration fixed, so
/// every `}`-then-`/` that must stay a division is pinned here — including the
/// two the plan named (`o.v / 2` → 4 and the `!function(){}()` IIFE → false).
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_176_object_literals_and_expressions_still_divide() {
    if !live_tests_enabled() {
        eprintln!("live_176_object_literals_and_expressions_still_divide: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_176_object_literals_and_expressions_still_divide");
    let port = ff.port();

    let cases: &[(&str, Value)] = &[
        // The plan's two must-not-regress lines.
        ("const o = {v:8}; o.v / 2", Value::from(4)),
        ("const r = !function(){ return 1 }(); r", Value::Bool(false)),
        // An object key, a nested object key and a ternary branch all put a
        // `{` after a `:` — none of them is a labelled block.
        ("const o = {a: {b: 7}}; o.a.b / 2", Value::from(3.5)),
        ("const o = {a: 1, b: {c: 2}}; o.b.c / 2", Value::from(1)),
        ("const o = {1: {a: 2}}; o[1].a / 2", Value::from(1)),
        ("const x = true ? {a: 4} : {b: 2}; x.a / 2", Value::from(2)),
        ("const x = false ? 9 : {b: 6}; x.b / 2", Value::from(3)),
        // A class *expression*'s body is not a statement block: a
        // ClassExpression is a PrimaryExpression, so this really is a division
        // and must not gain a boundary.
        ("const ce = class {} / 2", Value::Null),
        ("const ce = class K {} / 2", Value::Null),
        ("const ce = class K extends Object {} / 2", Value::Null),
        // The post-iter-170 review fix for function expressions is unchanged.
        ("const fe = function(){} / 2", Value::Null),
        // An arrow body is an expression's block: what follows on the same
        // statement must not be split off.
        ("const g = () => {}, h = 2; h", Value::from(2)),
        (
            "const s = [1,2].map(x => { return x*2 }).length / 2; s",
            Value::from(1),
        ),
        // A `switch`'s clause labels live inside the switch's own brace; only
        // the switch's `}` ends a statement.
        ("switch (2) { case 1: {} default: {} } 42", Value::from(42)),
        // iter-170's cases, re-pinned: the `${…}` interpolation and the plain
        // block `}` this iteration's lookback additions run alongside.
        (
            "const n = 1; if (n) {} /a;b/.test(\"a;b\")",
            Value::Bool(true),
        ),
        ("const t = `x${ {a:1}.a }y`; t", Value::from("x1y")),
    ];

    for (script, expected) in cases {
        assert_evaluates(port, script, expected);
    }

    stop_daemon(port);
}
