//! Live tests for iter-61r Theme C — eval `mapped.await` fix.
//!
//! Verifies that:
//! 1. `ff-rdp eval 'document.title'` returns the correct string on a page
//!    that has a strict Content Security Policy (Hacker News).
//! 2. The envelope carries no `meta.eval_path` key — iter-161 Theme E removed
//!    it (it had been the constant `"page-await"` since iter-93).
//!
//! # Running
//!
//! Requires Firefox, network access (news.ycombinator.com), and the ff-rdp
//! binary.  Gates on `FF_RDP_LIVE_NETWORK_TESTS=1`.
//!
//!   FF_RDP_LIVE_NETWORK_TESTS=1 cargo test -p ff-rdp-cli --test live live_61r_eval -- --nocapture

use std::process::{Command, Output};
use std::time::Duration;

use crate::common::{LiveFirefox, base_args, ff_rdp_bin};

/// The title news.ycombinator.com serves. A third-party constant: if the site
/// renames itself this test is *supposed* to fail, and say so as a site change
/// rather than as an `eval` defect — see the readiness wait below.
const HN_TITLE: &str = "Hacker News";

fn parse_json(output: &Output) -> serde_json::Value {
    let s = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(s.trim()).unwrap_or_else(|e| {
        panic!(
            "stdout is not valid JSON: {e}\nstdout={s}\nstderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

/// `live_eval_on_hn`: navigate to Hacker News (which has a CSP that blocks
/// `eval()`), then run `ff-rdp eval 'document.title'`.
///
/// Asserts:
/// - Exit code 0.
/// - `results` equals `"Hacker News"`.
/// - `meta` has no `eval_path` key (removed in iter-161 Theme E).
#[test]
#[ignore = "requires Firefox, network access (news.ycombinator.com), and FF_RDP_LIVE_NETWORK_TESTS=1"]
fn live_eval_on_hn() {
    if std::env::var("FF_RDP_LIVE_NETWORK_TESTS").is_err() {
        eprintln!("live_eval_on_hn: set FF_RDP_LIVE_NETWORK_TESTS=1 to run");
        return;
    }

    let ff = LiveFirefox::headless_on_random_port();

    let ff_args = || base_args(ff.port());

    // Navigate to Hacker News.
    let nav = Command::new(ff_rdp_bin())
        .args(ff_args())
        .args(["navigate", "https://news.ycombinator.com"])
        .output()
        .expect("navigate to HN");

    if !nav.status.success() {
        eprintln!(
            "live_eval_on_hn: navigate failed (network issue?) — {}",
            String::from_utf8_lossy(&nav.stderr)
        );
        return;
    }

    // iter-242 Part C: `navigate` returning success is not the same claim as
    // "the document is there". Iteration 175's closing sweep watched this test
    // read `document.title` as `""` seconds after a successful navigate, then
    // pass in isolation three minutes later — and asserting on the title alone
    // could not tell "not loaded yet" from "the site answered differently".
    // Wait for the document, bounded; whatever is still wrong afterwards gets
    // named rather than guessed.
    let doc = crate::common::await_document_ready(ff.port(), Duration::from_secs(20));
    assert!(
        doc.ready_state == "complete" && doc.title == HN_TITLE,
        "live_eval_on_hn: {}",
        doc.diagnosis(HN_TITLE)
    );

    // Evaluate `document.title` on the CSP-restricted page.
    let out = Command::new(ff_rdp_bin())
        .args(ff_args())
        .args(["eval", "document.title"])
        .output()
        .expect("eval document.title");

    assert!(
        out.status.success(),
        "live_eval_on_hn: eval exited non-zero — {}",
        crate::common::output_note(&out)
    );

    let json = parse_json(&out);

    // Hacker News title. The readiness wait above has already established
    // that the document reports this title, so a mismatch here is `eval`
    // returning something other than what the page holds — the guarantee this
    // test exists for — rather than a slow or changed site.
    assert_eq!(
        json["results"],
        serde_json::Value::String(HN_TITLE.to_owned()),
        "live_eval_on_hn: expected {HN_TITLE:?}, got: {}",
        json["results"]
    );

    // iter-161 Theme E: `meta.eval_path` was a constant and is gone. The
    // page-await path itself is unchanged — what this test proves is that
    // `eval` works on a strict-CSP page, which is the guarantee DEC-020
    // actually cares about; the envelope no longer restates it as a field.
    assert!(
        json["meta"].get("eval_path").is_none(),
        "live_eval_on_hn: meta.eval_path was removed in iter-161; got: {}",
        json["meta"]
    );
}
