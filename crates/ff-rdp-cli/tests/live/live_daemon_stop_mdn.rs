//! Live test for iter-95 Theme A: `daemon stop` on MDN-connected Firefox.
//!
//! AC: `live_daemon_stop_on_mdn_headless`
//! Reproduces session-60 §1: launch headless Firefox, navigate to MDN
//! (which exercises multi-process Firefox: GPU/RDD/content child processes
//! all hold the socket), then `daemon stop`. The port must be free within 15 s.
//!
//! Gated by `FF_RDP_LIVE_NETWORK_TESTS=1` (real network required).
//!
//! Run with:
//!   FF_RDP_LIVE_NETWORK_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live live_daemon_stop_mdn -- --nocapture

use std::process::Command;
use std::time::Duration;

use crate::common::live_network_tests_enabled;
use crate::common::{LiveFirefox, ff_rdp_bin};

/// Poll until `127.0.0.1:port` refuses connections (port is free) or timeout.
fn wait_port_free(port: u16, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if std::net::TcpStream::connect(format!("127.0.0.1:{port}")).is_err() {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// AC: `live_daemon_stop_on_mdn_headless`
///
/// **Reproducer for session-60 §1:** launch headless Firefox, navigate to
/// https://developer.mozilla.org (which spins up GPU/RDD/content child processes
/// all potentially inheriting the debug-port socket), then `ff-rdp daemon stop`.
///
/// Post-conditions:
/// - `daemon stop` command exits within 15 s.
/// - `is_port_in_use(port)` is false after the command completes.
///
/// On the iter-94 codebase (pre-fix) this test would fail: the port stayed held
/// by a child process after SIGKILL on the parent PID. The iter-95 fix adds a
/// SIGKILL on the captured PGID as a final escalation step.
///
/// Gated by `FF_RDP_LIVE_NETWORK_TESTS=1` — requires real network access.
/// Ignored by default so CI (which doesn't set this var) skips it.
#[test]
#[ignore = "requires live Firefox + network (MDN) — set FF_RDP_LIVE_NETWORK_TESTS=1"]
fn live_daemon_stop_on_mdn_headless() {
    if !live_network_tests_enabled() {
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_daemon_stop_on_mdn_headless: Firefox not available — skipping");
        return;
    };
    let port = ff.port();
    // Transfer ownership to ManuallyDrop so Drop doesn't kill Firefox before
    // `daemon stop` does — the test asserts the stop path reaps the process.
    let _keep = std::mem::ManuallyDrop::new(ff);

    eprintln!("live_daemon_stop_on_mdn_headless: Firefox up on port {port}");

    // Navigate to MDN — this causes Firefox to spin up content/GPU/RDD child
    // processes that inherit the debug-port socket (session-60 §1 reproducer).
    let navigate_out = Command::new(ff_rdp_bin())
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--no-daemon",
            "--allow-unsafe-urls",
            "--timeout",
            "30000",
            "navigate",
            "https://developer.mozilla.org",
        ])
        .output()
        .expect("live_daemon_stop_on_mdn_headless: failed to spawn navigate");

    if navigate_out.status.success() {
        eprintln!("live_daemon_stop_on_mdn_headless: navigate to MDN succeeded");
    } else {
        eprintln!(
            "live_daemon_stop_on_mdn_headless: navigate returned non-zero (continuing) — \
             stderr={}",
            String::from_utf8_lossy(&navigate_out.stderr)
        );
    }

    // Give Firefox a moment to settle (child processes spin up during navigation).
    std::thread::sleep(Duration::from_secs(2));

    assert!(
        std::net::TcpStream::connect(format!("127.0.0.1:{port}")).is_ok(),
        "live_daemon_stop_on_mdn_headless: port {port} must be open before daemon stop"
    );

    // `daemon stop` must complete within 15 s.
    let stop_start = std::time::Instant::now();
    let stop_out = Command::new(ff_rdp_bin())
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--timeout",
            "15000",
            "daemon",
            "stop",
        ])
        .output()
        .expect("live_daemon_stop_on_mdn_headless: failed to spawn daemon stop");
    let elapsed = stop_start.elapsed();

    eprintln!(
        "live_daemon_stop_on_mdn_headless: daemon stop completed in {:.1}s — stdout={}",
        elapsed.as_secs_f64(),
        String::from_utf8_lossy(&stop_out.stdout).trim()
    );

    assert!(
        elapsed < Duration::from_secs(15),
        "live_daemon_stop_on_mdn_headless: FAIL — daemon stop took {:.1}s, expected < 15s",
        elapsed.as_secs_f64()
    );

    assert!(
        stop_out.status.success(),
        "live_daemon_stop_on_mdn_headless: FAIL — daemon stop returned non-zero — \
         stderr={}",
        String::from_utf8_lossy(&stop_out.stderr)
    );

    // Port must be free immediately (or within a short poll window).
    let port_free = wait_port_free(port, Duration::from_secs(3));
    assert!(
        port_free,
        "live_daemon_stop_on_mdn_headless: FAIL — port {port} still listening \
         after daemon stop (iter-95 Theme A regression: child process held port after pgid kill)"
    );

    eprintln!(
        "live_daemon_stop_on_mdn_headless: PASS — port {port} free after daemon stop \
         in {:.1}s (MDN multi-process Firefox)",
        elapsed.as_secs_f64()
    );
}
