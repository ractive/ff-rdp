//! Live test for iter-142 Theme A: `daemon stop` pid honesty + per-port
//! isolation.
//!
//! AC: `live_142_daemon_stop_no_false_error`
//!
//! Root cause (confirmed against the filesystem, not just the plan's stated
//! symptom — see `daemon_record.rs`'s module doc comment):
//! `~/.ff-rdp/launch-record.json` was a single **global** file. Two
//! concurrent `launch`es on different ports clobbered each other's record —
//! the second `launch` overwrote the first's entry — so a later
//! `daemon stop --port A` read back a record for a *different* port, didn't
//! match, fell through to the proxy-daemon registry path, and reported the
//! **daemon's own PID** as if it were Firefox's (dogfooding session 63
//! item 27, reproduced 3/3 with four parallel agents on separate ports).
//! iter-142 scopes the record file per port
//! (`launch-record.<port>.json`) and, as defense in depth, makes the
//! registry-fallback path itself resolve and kill the real Firefox process
//! instead of the daemon's own PID (`daemon/client.rs::run_daemon_stop`).
//!
//! This test drives the exact repro shape: two `launch`es on different
//! ports (the default daemon-mode connection path throughout — this suite
//! never opts into a direct connection), then `daemon stop` on the first.
//!
//! Post-conditions:
//! - `daemon stop` exits 0
//! - `results.pid` equals the pid `launch` reported for that port (not the
//!   daemon's own pid, and not the *other* instance's pid)
//! - the stopped instance's port becomes free
//! - the untouched second instance stays alive (no cross-instance kill)
//!
//! Run with:
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live live_142_daemon_stop_pid_honesty -- --nocapture

use std::process::Command;
use std::time::Duration;

use crate::common::{LiveFirefox, ff_rdp_bin, kill_pid, live_tests_enabled};

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

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_142_daemon_stop_no_false_error() {
    if !live_tests_enabled() {
        return;
    }

    let Some(ff_a) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_142_daemon_stop_no_false_error: Firefox not available — skipping");
        return;
    };
    let port_a = ff_a.port();
    let pid_a = ff_a.pid();
    // Transfer ownership to ManuallyDrop so Drop doesn't kill instance A
    // before `daemon stop` does — the test asserts the stop path reaps it.
    let _keep_a = std::mem::ManuallyDrop::new(ff_a);

    let Some(ff_b) = LiveFirefox::headless_on_random_port() else {
        eprintln!(
            "live_142_daemon_stop_no_false_error: second Firefox not available — skipping \
             (cleaning up instance A manually)"
        );
        kill_pid(pid_a);
        return;
    };
    let port_b = ff_b.port();
    // ff_b's normal Drop kills it at the end of this function — it must
    // survive everything before that point, so no ManuallyDrop here.

    eprintln!(
        "live_142_daemon_stop_no_false_error: instance A pid={pid_a} port={port_a}, \
         instance B port={port_b} (both launched via the default daemon-mode path)"
    );

    // `daemon stop` on instance A only — instance B must be unaffected.
    let stop = Command::new(ff_rdp_bin())
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            &port_a.to_string(),
            "--timeout",
            "15000",
            "daemon",
            "stop",
        ])
        .output()
        .expect("live_142_daemon_stop_no_false_error: daemon stop spawn failed");

    assert!(
        stop.status.success(),
        "live_142_daemon_stop_no_false_error: daemon stop returned non-zero — \
         stdout={} stderr={}",
        String::from_utf8_lossy(&stop.stdout),
        String::from_utf8_lossy(&stop.stderr)
    );

    let json: serde_json::Value = serde_json::from_slice(&stop.stdout)
        .expect("live_142_daemon_stop_no_false_error: daemon stop JSON parse");
    let reported_pid = json["results"]["pid"].as_u64();
    assert_eq!(
        reported_pid,
        Some(u64::from(pid_a)),
        "live_142_daemon_stop_no_false_error: daemon stop must report the pid \
         `launch` returned for port {port_a} (pid {pid_a}), never the daemon's own \
         pid or instance B's pid — got {json}"
    );

    assert!(
        wait_port_free(port_a, Duration::from_secs(10)),
        "live_142_daemon_stop_no_false_error: FAIL — port {port_a} still listening \
         after daemon stop reported success"
    );

    assert!(
        std::net::TcpStream::connect(format!("127.0.0.1:{port_b}")).is_ok(),
        "live_142_daemon_stop_no_false_error: instance B on port {port_b} must survive \
         stopping instance A — a cross-instance kill would be the exact \
         global-launch-record clobber this iteration fixed"
    );

    eprintln!(
        "live_142_daemon_stop_no_false_error: PASS — pid {pid_a} reported and reaped, \
         port {port_a} freed, instance B on port {port_b} untouched"
    );
}
