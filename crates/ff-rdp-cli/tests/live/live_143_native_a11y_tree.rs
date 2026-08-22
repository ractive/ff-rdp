//! Live tests for iteration 143 — `meta.source` on `ff-rdp a11y` and the
//! opt-in `--native` flag (carry-over from
//! [[iteration-136-core-live-test-repairs]]).
//!
//! Firefox's platform accessibility service is off by default on a fresh
//! headless launch (iter-136), so a plain `ff-rdp a11y` against it exercises
//! the JS-derived fallback path and must report that honestly in
//! `meta.source`. `--native` opts in to the real platform tree for the
//! duration of one call and must restore the service to its previous
//! (disabled) state afterward — verified here by re-running a plain `a11y`
//! afterward and observing the fallback path again (DEC-027: the service
//! must not be left enabled behind the user's back).
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live live_143_native_a11y_tree -- --nocapture

use std::process::{Command, Output};

use serde_json::Value;

use crate::common::{LiveFirefox, base_args, ff_rdp_bin, live_tests_enabled};

fn parse_json(output: &Output) -> Value {
    let s = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(s.trim()).unwrap_or_else(|e| {
        panic!(
            "stdout is not valid JSON: {e}\nstdout={s}\nstderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn run_a11y(port: u16, extra: &[&str]) -> Value {
    let mut args = base_args(port);
    args.push("a11y".to_owned());
    args.extend(extra.iter().map(|s| (*s).to_owned()));
    let out = Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("ff-rdp a11y");
    assert!(
        out.status.success(),
        "ff-rdp a11y {extra:?} failed: {}",
        crate::common::output_note(&out)
    );
    parse_json(&out)
}

/// live_a11y_source_meta: `ff-rdp a11y` output carries a `meta.source` of
/// `js-fallback` against a Firefox with the accessibility service off.
#[test]
#[ignore = "requires Firefox + FF_RDP_LIVE_TESTS=1"]
fn live_a11y_source_meta() {
    if !live_tests_enabled() {
        eprintln!("live_a11y_source_meta: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }
    let ff = LiveFirefox::headless_on_random_port();

    let json = run_a11y(ff.port(), &[]);
    assert_eq!(
        json["meta"]["source"], "js-fallback",
        "a plain `a11y` call against a fresh headless Firefox (accessibility \
         service off by default) must report meta.source = js-fallback: {json}"
    );
    assert_eq!(
        json["meta"]["source_reason"], "accessibility-service-disabled",
        "the fallback reason must name why the native path was not used: {json}"
    );
}

/// live_a11y_native_opt_in: with the opt-in flag, the root role is
/// `document`, and the tree contains platform roles the JS fallback does not
/// produce.
#[test]
#[ignore = "requires Firefox + FF_RDP_LIVE_TESTS=1"]
fn live_a11y_native_opt_in() {
    if !live_tests_enabled() {
        eprintln!("live_a11y_native_opt_in: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }
    let ff = LiveFirefox::headless_on_random_port();

    let json = run_a11y(ff.port(), &["--native"]);
    assert_eq!(
        json["meta"]["source"], "native",
        "--native must report meta.source = native: {json}"
    );
    assert_eq!(
        json["results"]["role"], "document",
        "the native platform tree's root role must be \"document\" (not the \
         JS-derived fallback's DOM-approximated roles): {json}"
    );
    assert!(
        json["meta"].get("source_reason").is_none(),
        "a successful native run must not carry a fallback reason: {json}"
    );
}

/// live_a11y_service_restored: after an opt-in run that enabled the service,
/// `bootstrap().state.enabled` is back to its pre-run value.
///
/// There is no CLI surface for a raw `bootstrap()` probe, so this asserts the
/// externally observable equivalent: a plain (non-`--native`) `a11y` call
/// immediately after the opt-in run must take the JS-fallback path again —
/// proof the service did not stay enabled behind the caller's back.
#[test]
#[ignore = "requires Firefox + FF_RDP_LIVE_TESTS=1"]
fn live_a11y_service_restored() {
    if !live_tests_enabled() {
        eprintln!("live_a11y_service_restored: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }
    let ff = LiveFirefox::headless_on_random_port();

    // Pre-run: service is off by default on a fresh headless launch.
    let before = run_a11y(ff.port(), &[]);
    assert_eq!(before["meta"]["source"], "js-fallback");

    // Opt-in run: enables the service, walks the native tree, restores.
    let opted_in = run_a11y(ff.port(), &["--native"]);
    assert_eq!(opted_in["meta"]["source"], "native");

    // Post-run: must be back to js-fallback — the service must not have been
    // left enabled after the opt-in call returned.
    let after = run_a11y(ff.port(), &[]);
    assert_eq!(
        after["meta"]["source"], "js-fallback",
        "the accessibility service must be restored to disabled after a --native \
         run that turned it on — got meta={:?}",
        after["meta"]
    );
    assert_eq!(
        after["meta"]["source_reason"], "accessibility-service-disabled",
        "post-restore state must match the pre-run disabled state: {after}"
    );
}
