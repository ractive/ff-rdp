//! Live tests for iter-61r Theme C — eval `mapped.await` fix.
//!
//! Verifies that:
//! 1. `ff-rdp eval 'document.title'` returns the correct string on a page
//!    that has a strict Content Security Policy (Hacker News).
//! 2. The output includes `meta.eval_path: "page-await"` (standard path).
//!
//! # Running
//!
//! Requires Firefox, network access (news.ycombinator.com), and the ff-rdp
//! binary.  Gates on `FF_RDP_LIVE_NETWORK_TESTS=1`.
//!
//!   FF_RDP_LIVE_NETWORK_TESTS=1 cargo test -p ff-rdp-cli --test live live_61r_eval -- --nocapture

use std::process::{Command, Output};

use crate::common::{LiveFirefox, base_args, ff_rdp_bin};

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
/// - `meta.eval_path` equals `"page-await"` (standard path, not CSP fallback).
#[test]
#[ignore = "requires Firefox, network access (news.ycombinator.com), and FF_RDP_LIVE_NETWORK_TESTS=1"]
fn live_eval_on_hn() {
    if std::env::var("FF_RDP_LIVE_NETWORK_TESTS").is_err() {
        eprintln!("live_eval_on_hn: set FF_RDP_LIVE_NETWORK_TESTS=1 to run");
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_eval_on_hn: Firefox not available — skipping");
        return;
    };

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

    // Evaluate `document.title` on the CSP-restricted page.
    let out = Command::new(ff_rdp_bin())
        .args(ff_args())
        .args(["eval", "document.title"])
        .output()
        .expect("eval document.title");

    assert!(
        out.status.success(),
        "live_eval_on_hn: eval exited non-zero — stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json = parse_json(&out);

    // Hacker News title.
    assert_eq!(
        json["results"],
        serde_json::Value::String("Hacker News".to_owned()),
        "live_eval_on_hn: expected 'Hacker News', got: {}",
        json["results"]
    );

    // Standard page-await path must be reported.
    assert_eq!(
        json["meta"]["eval_path"], "page-await",
        "live_eval_on_hn: meta.eval_path must be 'page-await'; got: {}",
        json["meta"]["eval_path"]
    );
}
