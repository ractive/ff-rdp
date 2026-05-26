//! iter-82 AC: `live_navigate_dom_complete_within_default_timeout`.
//!
//! Navigates to a local HTTP fixture page that emits a 200ms delayed script
//! (simulating a slow SPA) and asserts the `navigate --wait-strategy both`
//! call returns within the default 10s budget with `ready_state == "complete"`.
//!
//! This validates Theme C: when events time out, the `both` strategy falls
//! back to polling `document.readyState` and succeeds.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live_navigate_real_site -- --nocapture

#[path = "common/mod.rs"]
mod common;

use std::process::Command;

use common::{LiveFirefox, base_args, ff_rdp_bin};

/// A minimal data URL page that fires a 200ms delayed JS mutation, then
/// sets `window.__ready = true`.  After the delay the DOM is complete.
const FIXTURE_URL: &str = "data:text/html;charset=utf-8,\
<!DOCTYPE html><html><head></head><body>\
<script>setTimeout(function(){window.__ready=true;},200);</script>\
</body></html>";

/// `live_navigate_dom_complete_within_default_timeout`:
/// Navigate to a fixture page with a 200ms delayed script and assert
/// `ff-rdp navigate --wait-strategy both` exits 0 within the default
/// timeout and reports `ready_state == "complete"`.
///
/// Gated on `FF_RDP_LIVE_TESTS=1`.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_navigate_dom_complete_within_default_timeout() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!(
            "live_navigate_dom_complete_within_default_timeout: set FF_RDP_LIVE_TESTS=1 to run"
        );
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!(
            "live_navigate_dom_complete_within_default_timeout: Firefox not available — skipping"
        );
        return;
    };

    let mut args = base_args(ff.port());
    args.extend([
        "navigate".to_owned(),
        FIXTURE_URL.to_owned(),
        "--wait-strategy".to_owned(),
        "both".to_owned(),
    ]);

    let start = std::time::Instant::now();
    let output = Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("failed to spawn ff-rdp navigate");
    let elapsed = start.elapsed();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "live_navigate_dom_complete_within_default_timeout: ff-rdp navigate must exit 0.\n\
         status={:?}\nstdout={stdout}\nstderr={stderr}",
        output.status,
    );

    assert!(
        elapsed.as_secs() < 10,
        "live_navigate_dom_complete_within_default_timeout: navigate took {elapsed:?} \
         which exceeds the 10s default budget"
    );

    let json: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
        panic!(
            "live_navigate_dom_complete_within_default_timeout: output is not valid JSON: \
                 {e}\nstdout={stdout}\nstderr={stderr}"
        )
    });

    // The navigate command may report ready_state directly or under results.
    let ready_state = json["results"]["ready_state"]
        .as_str()
        .or_else(|| json["ready_state"].as_str())
        .unwrap_or_default();
    assert_eq!(
        ready_state, "complete",
        "live_navigate_dom_complete_within_default_timeout: expected \
         ready_state=complete, got {ready_state:?}; full json={json}"
    );

    eprintln!(
        "live_navigate_dom_complete_within_default_timeout: PASS — \
         completed in {elapsed:?}, ready_state={ready_state:?}"
    );
}
