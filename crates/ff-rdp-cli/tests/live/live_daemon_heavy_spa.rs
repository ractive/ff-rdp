//! Live test for iter-61n Theme C — reader/dispatcher mpsc decoupling.
//!
//! Verifies that a new CLI connection can complete auth within 2 s even when
//! the daemon is processing a heavy burst of events (200 XHRs in a tight loop).
//!
//! Without the decoupling fix, the reader thread holds `rpc_writer` while
//! forwarding events, delaying auth-greeting writes and causing timeouts.
//!
//! # Running
//!
//! Requires Firefox and the ff-rdp binary.  Gates on `FF_RDP_LIVE_TESTS=1`.
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live live_daemon_heavy_spa -- --nocapture

use std::process::{Command, Output};
use std::time::{Duration, Instant};

use crate::common::{LiveFirefox, ff_rdp_bin};

fn parse_json(output: &Output) -> serde_json::Value {
    let s = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(s.trim()).unwrap_or_else(|e| {
        panic!(
            "stdout is not valid JSON: {e}\nstdout={s}\nstderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

/// `live_daemon_auth_completes_during_burst`:
/// Navigate to a data URL that fires 200 XHRs in a tight loop.  While those
/// events are flooding the daemon, a new CLI connection (`daemon status`) must
/// complete auth within 2 s.
///
/// Without the mpsc decoupling, the reader thread holds `rpc_writer` while
/// forwarding events, starving the auth-greeting write.
#[test]
#[ignore = "requires Firefox and FF_RDP_LIVE_TESTS=1"]
fn live_daemon_auth_completes_during_burst() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        eprintln!("live_daemon_auth_completes_during_burst: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_daemon_auth_completes_during_burst: Firefox not available — skipping");
        return;
    };

    let daemon_args = |timeout_ms: u64| {
        vec![
            "--host".to_owned(),
            "127.0.0.1".to_owned(),
            "--port".to_owned(),
            ff.port().to_string(),
            "--timeout".to_owned(),
            timeout_ms.to_string(),
        ]
    };

    // Trigger daemon startup with a first `eval` call. `tabs` does NOT work
    // for this: `tabs.rs` connects to Firefox directly via
    // `RdpConnection::connect` and never goes through
    // `resolve_connection_target`, so it never actually starts a daemon (see
    // the matching fix in `eval_object_leak_soak.rs`). `eval`, like
    // `navigate` below, routes through `connect_tab.rs` and genuinely
    // auto-starts and waits for the daemon to register.
    let init = Command::new(ff_rdp_bin())
        .args(daemon_args(5000))
        .args(["eval", "1"])
        .output()
        .expect("eval 1");
    if !init.status.success() {
        eprintln!("live_daemon_auth_completes_during_burst: daemon init failed — skipping");
        return;
    }
    // Give daemon a moment to be fully ready.
    std::thread::sleep(Duration::from_millis(500));

    // Build an XHR-burst page: 200 requests to data: URLs (no network needed).
    // The daemon will receive 200 network-event watcher messages in quick succession.
    let burst_html = r"data:text/html,<script>
(async () => {
  const N = 200;
  for (let i = 0; i < N; i++) {
    try {
      await fetch('data:text/plain,x' + i);
    } catch(_) {}
  }
})();
</script>";

    // Navigate to the burst page in a background thread so it fires XHRs.
    let ff_port = ff.port();
    let burst_thread = std::thread::spawn(move || {
        Command::new(ff_rdp_bin())
            .args([
                "--host",
                "127.0.0.1",
                "--port",
                &ff_port.to_string(),
                "--timeout",
                "15000",
                "navigate",
                burst_html,
                "--allow-unsafe-urls",
            ])
            .output()
    });

    // While the burst is in progress, try to connect a new CLI client.
    // The `daemon status` command establishes a fresh connection (auth + greeting).
    // We give it 2 s — if the reader/dispatcher decoupling is broken, this times out.
    std::thread::sleep(Duration::from_millis(300)); // Let the burst start.

    let auth_start = Instant::now();
    let status = Command::new(ff_rdp_bin())
        .args(daemon_args(2000))
        .args(["daemon", "status"])
        .output()
        .expect("daemon status");
    let auth_elapsed = auth_start.elapsed();

    // Wait for burst navigation to finish (or give up after 15 s).
    let _ = burst_thread.join();

    // Clean up daemon.
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

    let status_json = parse_json(&status);
    assert!(
        status.status.success(),
        "daemon status must succeed during burst (auth timed out or failed).\n\
         elapsed: {:?}\nstdout: {status_json}\nstderr: {}",
        auth_elapsed,
        String::from_utf8_lossy(&status.stderr)
    );

    assert!(
        auth_elapsed < Duration::from_secs(2),
        "auth + greeting must complete within 2 s during 200-XHR burst.\n\
         actual elapsed: {auth_elapsed:?}\n\
         This suggests the reader/dispatcher decoupling is not working."
    );

    assert_eq!(
        status_json["results"]["running"].as_bool(),
        Some(true),
        "daemon must report running=true"
    );

    eprintln!(
        "live_daemon_auth_completes_during_burst: PASSED — auth completed in {auth_elapsed:?}"
    );
}
