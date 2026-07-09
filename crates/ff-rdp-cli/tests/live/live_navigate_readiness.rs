//! iter-79 Theme A AC: `ff-rdp navigate` against a real public URL must succeed
//! under the default `--wait complete` + default `--timeout`.
//!
//! Reproduces the dogfooding repro (2026-05-25): pre-iter-79 the command timed
//! out at the default 10s and at 30s even though `eval document.readyState`
//! returned `"complete"`. Post-fix the watcher's frame-target stream is engaged
//! before navigateTo, so document-event resources flow and `wait_for_doc_complete`
//! observes `dom-complete`.
//!
//! Gated on `FF_RDP_LIVE_NETWORK_TESTS=1` because it hits a live public site.
//!
//!   FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 \
//!     cargo test -p ff-rdp-cli --test live live_navigate_readiness -- --ignored

use std::process::Command;

use crate::common::{LiveFirefox, base_args, ff_rdp_bin};

#[test]
#[ignore = "requires Firefox, network, FF_RDP_LIVE_TESTS=1 and FF_RDP_LIVE_NETWORK_TESTS=1"]
fn live_navigate_default_wait_reaches_complete() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_navigate_default_wait_reaches_complete: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }
    if std::env::var("FF_RDP_LIVE_NETWORK_TESTS").is_err() {
        eprintln!(
            "live_navigate_default_wait_reaches_complete: set FF_RDP_LIVE_NETWORK_TESTS=1 to run"
        );
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_navigate_default_wait_reaches_complete: Firefox not available — skipping");
        return;
    };

    let url = "https://tennis-sepp.ch";

    let mut args = base_args(ff.port());
    args.extend(["navigate".to_owned(), url.to_owned()]);

    let output = Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp navigate");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "ff-rdp navigate {url} must exit 0 under the default wait/timeout.\nstatus={:?}\nstdout={stdout}\nstderr={stderr}",
        output.status,
    );

    let json: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
        panic!("navigate output is not valid JSON: {e}\nstdout={stdout}\nstderr={stderr}")
    });

    let committed = json["results"]["committed_url"]
        .as_str()
        .or_else(|| json["committed_url"].as_str())
        .unwrap_or_default();
    assert!(
        !committed.is_empty(),
        "navigate result must report a non-empty committed_url; got {json}"
    );

    let ready_state = json["results"]["ready_state"]
        .as_str()
        .or_else(|| json["ready_state"].as_str())
        .unwrap_or_default();
    assert_eq!(
        ready_state, "complete",
        "default --wait must reach ready_state=complete; got {json}"
    );
}
