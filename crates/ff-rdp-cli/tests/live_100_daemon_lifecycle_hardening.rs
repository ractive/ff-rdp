//! Live tests for iter-100: daemon lifecycle hardening.
//!
//! Theme C — signal-driven registry cleanup:
//!   * `e2e_sigterm_removes_registry`: after SIGTERM the daemon removes its
//!     registry file (which contains the auth token) before exiting, instead
//!     of leaving it behind (the pre-fix `setup_signal_handler` was a no-op).
//!
//! Run with:
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live_100_daemon_lifecycle_hardening -- --nocapture

#[path = "common/mod.rs"]
mod common;

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use common::{LiveFirefox, ff_rdp_bin};

fn live_tests_enabled() -> bool {
    std::env::var("FF_RDP_LIVE_TESTS").as_deref() == Ok("1")
}

/// Path to the daemon registry file inside an isolated `FF_RDP_HOME`.
fn registry_path(home: &std::path::Path) -> PathBuf {
    home.join(".ff-rdp").join("daemon.json")
}

/// Auto-start a daemon for `port` inside an isolated `FF_RDP_HOME` and return
/// its PID (read from `daemon status`).
fn autostart_daemon(home: &std::path::Path, port: u16) -> Option<u32> {
    // A tabs call without --no-daemon auto-starts the daemon.
    let init = Command::new(ff_rdp_bin())
        .env("FF_RDP_HOME", home)
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--timeout",
            "10000",
            "tabs",
        ])
        .output()
        .ok()?;
    if !init.status.success() {
        eprintln!(
            "autostart_daemon: tabs failed: {}",
            String::from_utf8_lossy(&init.stderr)
        );
        return None;
    }

    // Poll daemon status for a pid.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let status = Command::new(ff_rdp_bin())
            .env("FF_RDP_HOME", home)
            .args([
                "--host",
                "127.0.0.1",
                "--port",
                &port.to_string(),
                "daemon",
                "status",
            ])
            .output()
            .ok()?;
        if status.status.success()
            && let Ok(json) = serde_json::from_slice::<serde_json::Value>(&status.stdout)
            && let Some(pid) = json["results"]["pid"]
                .as_u64()
                .and_then(|p| u32::try_from(p).ok())
        {
            return Some(pid);
        }
        if std::time::Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// `e2e_sigterm_removes_registry`: SIGTERM to the daemon removes the registry
/// file (auth token) before the process exits.
///
/// Pre-fix: `setup_signal_handler` was a no-op, so the runtime's default
/// SIGTERM behaviour terminated the daemon immediately, leaving the registry
/// file (and its auth token) on disk. Post-fix: the handler flips the shutdown
/// flag, the accept loop returns, and `run_daemon` runs `remove_registry`.
#[test]
#[cfg(unix)]
#[ignore = "requires Firefox and FF_RDP_LIVE_TESTS=1"]
fn e2e_sigterm_removes_registry() {
    if !live_tests_enabled() {
        eprintln!("e2e_sigterm_removes_registry: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("e2e_sigterm_removes_registry: Firefox not available — skipping");
        return;
    };

    let home = tempfile::tempdir().expect("tempdir for FF_RDP_HOME");

    let Some(daemon_pid) = autostart_daemon(home.path(), ff.port()) else {
        panic!("e2e_sigterm_removes_registry: daemon never reported a pid");
    };
    eprintln!("e2e_sigterm_removes_registry: daemon pid={daemon_pid}");

    let reg = registry_path(home.path());
    assert!(
        reg.exists(),
        "precondition: registry file must exist while the daemon runs ({})",
        reg.display()
    );

    // Send SIGTERM to the daemon process (not the group — just the daemon).
    // SAFETY: kill(pid, SIGTERM) has no memory-safety implications.
    #[allow(clippy::cast_possible_wrap)]
    unsafe {
        libc::kill(daemon_pid as libc::pid_t, libc::SIGTERM);
    }

    // Wait for the daemon to observe the signal, run cleanup, and exit.
    // The accept loop polls the flag every ~100ms; give it a generous budget.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let mut registry_gone = false;
    let mut process_gone = false;
    while std::time::Instant::now() < deadline {
        registry_gone = !reg.exists();
        // kill(pid, 0) == -1/ESRCH once the process is gone.
        #[allow(clippy::cast_possible_wrap)]
        let alive = unsafe { libc::kill(daemon_pid as libc::pid_t, 0) } == 0;
        process_gone = !alive;
        if registry_gone && process_gone {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    assert!(
        registry_gone,
        "SIGTERM must remove the registry file before exit ({} still present)",
        reg.display()
    );
    assert!(
        process_gone,
        "daemon process (pid {daemon_pid}) must exit cleanly after SIGTERM"
    );

    eprintln!("e2e_sigterm_removes_registry: PASS — registry removed and daemon exited");
    // `ff` (Firefox) is cleaned up by LiveFirefox::drop.
}
