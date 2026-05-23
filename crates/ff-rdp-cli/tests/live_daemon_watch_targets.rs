//! Live test for iter-61n Theme A — `watchTargets("frame")` engagement.
//!
//! # Running
//!
//! Requires Firefox and the ff-rdp binary.  Gates on `FF_RDP_LIVE_TESTS=1`.
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live_daemon_watch_targets -- --nocapture

#[path = "common/mod.rs"]
mod common;

use std::process::{Command, Output};
use std::time::Duration;

use common::{LiveFirefox, ff_rdp_bin};

fn parse_json(output: &Output) -> serde_json::Value {
    let s = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(s.trim()).unwrap_or_else(|e| {
        panic!(
            "stdout is not valid JSON: {e}\nstdout={s}\nstderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

// ---------------------------------------------------------------------------
// Helper: start a daemon for this Firefox instance and return its proxy port.
// ---------------------------------------------------------------------------

fn start_daemon_for(ff: &LiveFirefox) -> Option<u16> {
    ff.with_daemon()
}

/// `live_daemon_watch_targets`:
/// Navigate to two data URLs back-to-back.  The daemon should receive
/// `target-available-form` events and increment `target_count` in `daemon status`.
///
/// Asserts: `daemon status` shows `target_count >= 1` after the navigations.
#[test]
#[ignore = "requires Firefox and FF_RDP_LIVE_TESTS=1"]
fn live_daemon_watch_targets() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_daemon_watch_targets: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_daemon_watch_targets: Firefox not available — skipping");
        return;
    };

    // Trigger daemon startup.
    if start_daemon_for(&ff).is_none() {
        eprintln!("live_daemon_watch_targets: daemon did not start — skipping");
        // Stop any partial daemon.
        let _ = Command::new(ff_rdp_bin())
            .args([
                "--host",
                "127.0.0.1",
                "--port",
                &ff.port().to_string(),
                "daemon",
                "stop",
            ])
            .output();
        return;
    }

    let daemon_args = || {
        vec![
            "--host".to_owned(),
            "127.0.0.1".to_owned(),
            "--port".to_owned(),
            ff.port().to_string(),
            "--timeout".to_owned(),
            "10000".to_owned(),
        ]
    };

    // Navigate to first page.
    let nav1 = Command::new(ff_rdp_bin())
        .args(daemon_args())
        .args([
            "navigate",
            "data:text/html,<h1>Page A</h1>",
            "--allow-unsafe-urls",
        ])
        .output()
        .expect("nav1");
    if !nav1.status.success() {
        eprintln!("live_daemon_watch_targets: nav1 failed; skipping");
        let _ = Command::new(ff_rdp_bin())
            .args([
                "--host",
                "127.0.0.1",
                "--port",
                &ff.port().to_string(),
                "daemon",
                "stop",
            ])
            .output();
        return;
    }

    // Navigate to second page (cross-origin from first).
    let nav2 = Command::new(ff_rdp_bin())
        .args(daemon_args())
        .args([
            "navigate",
            "data:text/html,<h1>Page B</h1>",
            "--allow-unsafe-urls",
        ])
        .output()
        .expect("nav2");
    if !nav2.status.success() {
        eprintln!("live_daemon_watch_targets: nav2 failed; skipping");
        let _ = Command::new(ff_rdp_bin())
            .args([
                "--host",
                "127.0.0.1",
                "--port",
                &ff.port().to_string(),
                "daemon",
                "stop",
            ])
            .output();
        return;
    }

    // Give the daemon a moment to process the target events.
    std::thread::sleep(Duration::from_millis(500));

    // Check daemon status — target_count should be >= 1.
    let status = Command::new(ff_rdp_bin())
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            &ff.port().to_string(),
            "daemon",
            "status",
        ])
        .output()
        .expect("daemon status");
    let status_json = parse_json(&status);
    let target_count = status_json["results"]["target_count"].as_u64().unwrap_or(0);

    // Clean up daemon before asserting.
    let _ = Command::new(ff_rdp_bin())
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            &ff.port().to_string(),
            "daemon",
            "stop",
        ])
        .output();

    assert!(
        target_count >= 1,
        "daemon target_count should be >= 1 after navigations (got {target_count}); \
         watchTargets('frame') may not be engaged.\n\
         daemon status: {status_json}"
    );

    eprintln!("live_daemon_watch_targets: PASSED — target_count={target_count}");
}
