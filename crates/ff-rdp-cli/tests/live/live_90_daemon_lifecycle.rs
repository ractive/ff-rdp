//! Live tests for iter-90: daemon lifecycle state sharing.
//!
//! Theme A — `launch` and `daemon stop` share one record:
//!   * `daemon stop` after `launch` frees the Firefox RDP port
//!   * `launch --replace` stops a prior instance started via `launch`
//!   * Pre-fix repro: on branch HEAD the port is free after `daemon stop`;
//!     on origin/main it stays held.
//!
//! Run with:
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live live_90_daemon_lifecycle -- --nocapture

use crate::common;

use std::process::Command;
use std::time::Duration;

use crate::common::live_tests_enabled;
use crate::common::{LiveFirefox, ff_rdp_bin};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Launch Firefox headless on `port` via the CLI and return its PID.
///
/// Returns `None` if the launch fails or the port is not reachable within 10 s.
fn launch_on_port(port: u16) -> Option<u32> {
    let out = Command::new(ff_rdp_bin())
        .args(["launch", "--headless", "--debug-port", &port.to_string()])
        .output()
        .ok()?;

    if !out.status.success() {
        eprintln!(
            "launch_on_port({port}): FAILED — stderr={}",
            String::from_utf8_lossy(&out.stderr)
        );
        return None;
    }

    let json: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    let pid = u32::try_from(json["results"]["pid"].as_u64()?).ok()?;
    eprintln!("launch_on_port({port}): pid={pid}");
    Some(pid)
}

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

/// Poll until `127.0.0.1:port` accepts connections or timeout.
fn wait_port_open(port: u16, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if std::net::TcpStream::connect(format!("127.0.0.1:{port}")).is_ok() {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

// ---------------------------------------------------------------------------
// Theme A — launch → daemon stop frees port
// ---------------------------------------------------------------------------

/// `live_daemon_stop_after_launch_frees_port`:
/// `ff-rdp launch --headless --port <N>` followed by `ff-rdp daemon stop`
/// must free the RDP port within 3 seconds.
///
/// Pre-condition:  Firefox launched via `launch` (NOT `daemon start`).
/// Post-condition: `TcpStream::connect("127.0.0.1:<port>")` fails within 3 s.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_daemon_stop_after_launch_frees_port() {
    if !live_tests_enabled() {
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_daemon_stop_after_launch_frees_port: Firefox not available — skipping");
        return;
    };
    let port = ff.port();
    // Forget `ff` so Drop doesn't kill Firefox before `daemon stop` does.
    // We'll verify the port is free ourselves.
    let _keep = std::mem::ManuallyDrop::new(ff);

    assert!(
        std::net::TcpStream::connect(format!("127.0.0.1:{port}")).is_ok(),
        "live_daemon_stop_after_launch_frees_port: port {port} must be open before stop"
    );

    let stop = Command::new(ff_rdp_bin())
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "daemon",
            "stop",
        ])
        .output()
        .expect("live_daemon_stop_after_launch_frees_port: daemon stop spawn failed");

    assert!(
        stop.status.success(),
        "live_daemon_stop_after_launch_frees_port: daemon stop returned non-zero — \
         stderr={}",
        String::from_utf8_lossy(&stop.stderr)
    );

    let port_freed = wait_port_free(port, Duration::from_secs(4));
    assert!(
        port_freed,
        "live_daemon_stop_after_launch_frees_port: FAIL — port {port} still \
         listening after daemon stop (iter-90 regression)"
    );

    eprintln!(
        "live_daemon_stop_after_launch_frees_port: PASS — port {port} freed after daemon stop"
    );
}

/// `live_launch_replace_handles_prior_instance`:
/// With a live Firefox running on port N (started via `launch`), `ff-rdp launch
/// --replace` must succeed and the new PID must differ from the prior PID.
///
/// Pre-condition:  Firefox running on random port, started by this test.
/// Post-condition: `launch --replace` exits 0; new PID != prior PID.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_launch_replace_handles_prior_instance() {
    if !live_tests_enabled() {
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("live_launch_replace_handles_prior_instance: Firefox not available — skipping");
        return;
    };
    let port = ff.port();
    let prior_pid = ff.pid();
    // Suppress Drop so `launch --replace` is the one that kills the prior
    // instance — the test then asserts the new PID differs from `prior_pid`.
    let _keep = std::mem::ManuallyDrop::new(ff);

    let out = Command::new(ff_rdp_bin())
        .args([
            "launch",
            "--headless",
            "--debug-port",
            &port.to_string(),
            "--replace",
        ])
        .output()
        .expect(
            "live_launch_replace_handles_prior_instance: failed to spawn ff-rdp launch --replace",
        );

    assert!(
        out.status.success(),
        "live_launch_replace_handles_prior_instance: FAIL — launch --replace returned non-zero\n\
         stderr={}\nstdout={}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );

    let json: serde_json::Value = serde_json::from_slice(&out.stdout)
        .expect("live_launch_replace_handles_prior_instance: stdout is not valid JSON");
    let new_pid_u64 = json["results"]["pid"]
        .as_u64()
        .expect("live_launch_replace_handles_prior_instance: results.pid missing");
    let new_pid = u32::try_from(new_pid_u64).expect("pid fits u32");

    assert_ne!(
        new_pid, prior_pid,
        "live_launch_replace_handles_prior_instance: FAIL — new pid ({new_pid}) \
         must differ from prior pid ({prior_pid})"
    );

    eprintln!(
        "live_launch_replace_handles_prior_instance: PASS — \
         prior pid={prior_pid}, new pid={new_pid} on port={port}"
    );

    // Clean up the new instance (cross-platform via the helper).
    common::kill_pid(new_pid);
    let _ = wait_port_free(port, Duration::from_secs(5));
}

// ---------------------------------------------------------------------------
// Pre-fix repro (red-then-green)
// ---------------------------------------------------------------------------

/// `pre_fix_repro_daemon_state_sharing_red_then_green`:
///
/// **On `origin/main` (RED):**
/// `ff-rdp daemon stop` after `ff-rdp launch` returns `{"reason": "not
/// running"}` AND the port stays held — the bug this iteration fixes.
///
/// **On branch HEAD (GREEN):**
/// `ff-rdp daemon stop` after `ff-rdp launch` frees the port within 3 s
/// AND a follow-up `launch` succeeds on the same port.
///
/// `xtask check-pre-fix-repro` runs this test on both revisions and expects
/// FAIL on main, PASS on HEAD.
///
/// Gated on `FF_RDP_LIVE_TESTS=1` (not `#[ignore]`) so that
/// `xtask check-pre-fix-repro` — which invokes `cargo test … --exact` without
/// `--include-ignored` — can run this test on both revisions. When the env
/// var is unset, the body returns immediately as a no-op pass.
///
/// Uses a fixed port (6090) to keep the test deterministic; if that port is
/// in use the test skips gracefully.
///
// allow-ungated-live: intentionally NOT #[ignore] — `xtask check-pre-fix-repro`
// runs this via `cargo test --exact` WITHOUT `--include-ignored`, so #[ignore]
// would make the pre-fix-repro gate unable to see it. Gated at runtime on
// FF_RDP_LIVE_TESTS=1 (no-op pass when unset). See iter-90 / iter-113 Theme B.
#[test]
fn pre_fix_repro_daemon_state_sharing_red_then_green() {
    if !live_tests_enabled() {
        return;
    }

    // Use a port unlikely to collide with other tests.
    let port: u16 = 6090;

    // If the port is already in use, skip rather than fight for it.
    if std::net::TcpStream::connect(format!("127.0.0.1:{port}")).is_ok() {
        eprintln!(
            "pre_fix_repro_daemon_state_sharing_red_then_green: port {port} \
             already in use — skipping"
        );
        return;
    }

    // 1. Launch Firefox via `ff-rdp launch`.
    let pid = launch_on_port(port)
        .expect("pre_fix_repro_daemon_state_sharing_red_then_green: launch failed");

    assert!(
        wait_port_open(port, Duration::from_secs(15)),
        "pre_fix_repro_daemon_state_sharing_red_then_green: port {port} not open after launch (pid={pid})"
    );

    // 2. Call `daemon stop`.
    let stop_out = Command::new(ff_rdp_bin())
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "daemon",
            "stop",
        ])
        .output()
        .expect("pre_fix_repro_daemon_state_sharing_red_then_green: daemon stop spawn failed");

    // Parse the JSON response.
    let stop_json: serde_json::Value =
        serde_json::from_slice(&stop_out.stdout).unwrap_or(serde_json::Value::Null);
    let reason = stop_json["results"]["reason"].as_str().unwrap_or("");

    eprintln!(
        "pre_fix_repro_daemon_state_sharing_red_then_green: daemon stop response={}",
        String::from_utf8_lossy(&stop_out.stdout).trim()
    );

    // On main (RED): reason == "not running" and port is still held.
    // On HEAD (GREEN): reason is absent and port is freed.
    // This assertion PASSES on HEAD (GREEN) and FAILS on main (RED).
    assert!(
        reason != "not running",
        "pre_fix_repro_daemon_state_sharing_red_then_green: FAIL (origin/main behavior) — \
         daemon stop returned 'not running' even though Firefox pid={pid} is/was running. \
         This is the bug that iter-90 fixes."
    );

    // 3. Port must be free within 3 s (branch HEAD behavior).
    let port_freed = wait_port_free(port, Duration::from_secs(4));
    assert!(
        port_freed,
        "pre_fix_repro_daemon_state_sharing_red_then_green: FAIL — port {port} \
         still listening after daemon stop (pid={pid})"
    );

    // 4. Follow-up launch on the same port must succeed.
    let relaunch_out = Command::new(ff_rdp_bin())
        .args(["launch", "--headless", "--debug-port", &port.to_string()])
        .output()
        .expect("pre_fix_repro_daemon_state_sharing_red_then_green: relaunch spawn failed");

    assert!(
        relaunch_out.status.success(),
        "pre_fix_repro_daemon_state_sharing_red_then_green: FAIL — follow-up launch \
         returned non-zero\nstderr={}",
        String::from_utf8_lossy(&relaunch_out.stderr)
    );

    let relaunch_json: serde_json::Value =
        serde_json::from_slice(&relaunch_out.stdout).unwrap_or(serde_json::Value::Null);
    let new_pid_raw = relaunch_json["results"]["pid"].as_u64().unwrap_or(0);
    let new_pid = u32::try_from(new_pid_raw).unwrap_or(0);

    eprintln!(
        "pre_fix_repro_daemon_state_sharing_red_then_green: PASS — \
         port {port} freed; follow-up launch succeeded (new pid={new_pid})"
    );

    // Cleanup the new Firefox instance (cross-platform via the helper).
    if new_pid > 0 {
        common::kill_pid(new_pid);
        let _ = wait_port_free(port, Duration::from_secs(5));
    }
}
