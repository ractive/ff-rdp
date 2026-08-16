//! Live tests for iteration 165 — "`eval`'s `const`/`let` bindings leak across
//! calls, contradicting `eval --help`".
//!
//! `eval --help` promised since iter-93 that every call has its own scope and
//! that `const`/`let` never leak. It was not true on the plain synchronous
//! path: that path sent the user's script to `Debugger.evalInGlobal` verbatim,
//! and that call evaluates in the tab's *own* global lexical environment, so
//! the second `ff-rdp eval 'const x = 1; x'` against one tab died with
//! `redeclaration of const x`. iter-165 wraps the plain path in the same
//! per-call IIFE `--stringify` (iter-161) and top-level `await` (iter-132)
//! already used, and revives `--no-isolate` as the documented opt-out.
//!
//! Firefox is the only JS engine this repo may use as ground truth (all code
//! stays in Rust — there is no in-process JS parser to check the wrap
//! against), so the contract is asserted here rather than in a unit test.
//!
//! daemon-parity: these use the default daemon path, like every real
//! invocation.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live live_165_eval_call_scope -- --nocapture

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

/// Run `ff-rdp eval <flags> <script>` and return `results`, asserting exit 0.
fn eval_ok(port: u16, script: &str, flags: &[&str]) -> Value {
    let mut args = vec!["eval"];
    args.extend_from_slice(flags);
    args.push(script);
    let out = run(port, &args);
    assert!(
        out.status.success(),
        "eval {flags:?} {script:?} must exit 0; got: {}",
        combined(&out)
    );
    parse_json(&out, &args)["results"].clone()
}

// ---------------------------------------------------------------------------
// AC live_165_repeated_const_matches_help
// ---------------------------------------------------------------------------

/// AC: `live_165_repeated_const_matches_help`.
///
/// The exact `dogfood_path` repro. Measured on main 2026-08-16: the first call
/// printed `1`, the second exited 1 with
/// `{"error":"redeclaration of const x","error_type":"User"}`. Both calls must
/// now exit 0 and both must print `1` — that is what `eval --help` promises,
/// and it is what makes `ff-rdp eval` idempotent in a loop.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_165_repeated_const_matches_help() {
    if !live_tests_enabled() {
        eprintln!("live_165_repeated_const_matches_help: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_165_repeated_const_matches_help");
    let port = ff.port();

    for attempt in 1..=3 {
        let results = eval_ok(port, "const x = 1; x", &[]);
        assert_eq!(
            results,
            Value::from(1),
            "call {attempt} of `eval 'const x = 1; x'` must return 1"
        );
    }

    // The same must hold for `let` — a separate binding kind in the spec, and
    // the plan explicitly refused to assume the two behave alike.
    for attempt in 1..=3 {
        let results = eval_ok(port, "let y = 1; y", &[]);
        assert_eq!(
            results,
            Value::from(1),
            "call {attempt} of `eval 'let y = 1; y'` must return 1"
        );
    }

    stop_daemon(port);
}

// ---------------------------------------------------------------------------
// AC live_165_scope_behaviour_table
// ---------------------------------------------------------------------------

/// AC: `live_165_scope_behaviour_table`.
///
/// Pins the whole table recorded in
/// `kb/iterations/iteration-165-eval-scope-leak-contradicts-help.md`:
/// `const`, `let`, `var` and `class` across two consecutive `eval` calls
/// against ONE tab, on both the plain and the `--stringify` path, plus the
/// two deliberate escapes — a bare assignment (which writes a property on the
/// page global and is meant to survive) and `--no-isolate` (which restores the
/// pre-165 shared scope).
///
/// One Firefox, one tab, no reload between cases: the shared global is the
/// whole point.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_165_scope_behaviour_table() {
    if !live_tests_enabled() {
        eprintln!("live_165_scope_behaviour_table: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_165_scope_behaviour_table");
    let port = ff.port();

    // (script, binding name) — each runs twice on the plain path and twice on
    // the --stringify path. Distinct names per path so the two paths cannot
    // mask each other's leak.
    let plain_cases = [
        ("const c1 = 1; c1", "c1"),
        ("let l1 = 1; l1", "l1"),
        ("var v1 = 1; v1", "v1"),
        ("class K1 { get n() { return 1 } }; new K1().n", "K1"),
    ];
    for (script, binding) in plain_cases {
        for attempt in 1..=2 {
            let results = eval_ok(port, script, &[]);
            assert_eq!(
                results,
                Value::from(1),
                "plain path, call {attempt} of {script:?} must return 1"
            );
        }
        // The binding must not have escaped into the tab's global scope.
        let seen = eval_ok(port, &format!("typeof {binding}"), &[]);
        assert_eq!(
            seen,
            Value::from("undefined"),
            "{binding} declared by {script:?} leaked out of its call scope"
        );
    }

    let stringify_cases = [
        ("const c2 = 1; c2", "c2"),
        ("let l2 = 1; l2", "l2"),
        ("var v2 = 1; v2", "v2"),
        ("class K2 { get n() { return 1 } }; new K2().n", "K2"),
    ];
    for (script, binding) in stringify_cases {
        for attempt in 1..=2 {
            let results = eval_ok(port, script, &["--stringify"]);
            assert_eq!(
                results,
                Value::from(1),
                "--stringify path, call {attempt} of {script:?} must return 1"
            );
        }
        let seen = eval_ok(port, &format!("typeof {binding}"), &[]);
        assert_eq!(
            seen,
            Value::from("undefined"),
            "{binding} declared by --stringify {script:?} leaked out of its call scope"
        );
    }

    // A bare assignment is a property write on the page global, not a
    // declaration — it is the documented way to publish state deliberately
    // and must keep working across calls.
    assert_eq!(eval_ok(port, "w1 = 1; w1", &[]), Value::from(1));
    assert_eq!(eval_ok(port, "w1 = 1; w1", &[]), Value::from(1));
    assert_eq!(
        eval_ok(port, "typeof w1", &[]),
        Value::from("number"),
        "a bare assignment must still reach the page global"
    );
    assert_eq!(
        eval_ok(port, "window.mine = 2; window.mine", &[]),
        Value::from(2)
    );
    assert_eq!(
        eval_ok(port, "typeof window.mine", &[]),
        Value::from("number"),
        "an explicit window property must survive across calls"
    );

    // --no-isolate is the opt-out: it restores the pre-165 shared scope, so
    // the SECOND identical call is expected to fail with a redeclaration
    // error. Asserting the failure is what makes the flag's help honest.
    let first = run(port, &["eval", "--no-isolate", "const n1 = 1; n1"]);
    assert!(
        first.status.success(),
        "first --no-isolate call must succeed: {}",
        combined(&first)
    );
    let second = run(port, &["eval", "--no-isolate", "const n1 = 1; n1"]);
    let text = combined(&second);
    assert!(
        !second.status.success() && text.contains("redeclaration"),
        "--no-isolate must share one scope, so the second identical call must \
         fail with a redeclaration error; got: {text}"
    );
    // And the binding it created IS visible to a later call — that is the
    // whole reason the flag exists.
    assert_eq!(
        eval_ok(port, "typeof n1", &[]),
        Value::from("number"),
        "--no-isolate must leave its declaration in the tab's global scope"
    );

    stop_daemon(port);
}

// ---------------------------------------------------------------------------
// The two documented consequences of the wrap
// ---------------------------------------------------------------------------

/// The plain path's completion-value semantics change in exactly two ways,
/// both now stated in `eval --help`. Pinning them here stops the help from
/// drifting back into a claim Firefox does not honour.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_165_wrap_completion_value_consequences() {
    if !live_tests_enabled() {
        eprintln!("live_165_wrap_completion_value_consequences: set FF_RDP_LIVE_TESTS=1");
        return;
    }
    let ff = firefox_with_daemon("live_165_wrap_completion_value_consequences");
    let port = ff.port();

    // (1) `return` at top level used to be `SyntaxError: illegal return
    //     statement`; inside the per-call IIFE it is legal and returns.
    assert_eq!(
        eval_ok(port, "return 1", &[]),
        Value::from(1),
        "an explicit top-level return must now surface its value"
    );

    // (2) A trailing control-flow construct no longer yields its script
    //     completion value — the help says to add an explicit `return` or to
    //     pass --no-isolate.
    assert_eq!(
        eval_ok(port, "if (1) { 2 }", &[]),
        serde_json::json!({"type": "undefined"}),
        "a trailing control-flow construct has no auto-returnable value"
    );
    assert_eq!(
        eval_ok(port, "if (1) { return 2 }", &[]),
        Value::from(2),
        "the documented workaround — an explicit return — must work"
    );
    assert_eq!(
        eval_ok(port, "if (1) { 2 }", &["--no-isolate"]),
        Value::from(2),
        "the other documented workaround — --no-isolate — must restore the \
         script completion value"
    );

    stop_daemon(port);
}
