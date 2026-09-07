//! Live tests for iter-100: daemon lifecycle hardening.
//!
//! Theme C — signal-driven registry cleanup:
//!   * `e2e_sigterm_removes_registry`: after SIGTERM the daemon removes its
//!     registry file (which contains the auth token) before exiting, instead
//!     of leaving it behind (the pre-fix `setup_signal_handler` was a no-op).
//!
//! Run with:
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live live_100_daemon_lifecycle_hardening -- --nocapture

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use crate::common::live_tests_enabled;
use crate::common::{LiveFirefox, ff_rdp_bin};

/// Path to the daemon registry file for `port` inside an isolated
/// `FF_RDP_HOME`.  iter-123 Theme B keys the registry by Firefox port, so the
/// file is `daemon.<port>.json` (not the old single-slot `daemon.json`).
fn registry_path(home: &std::path::Path, port: u16) -> PathBuf {
    home.join(".ff-rdp").join(format!("daemon.{port}.json"))
}

/// Auto-start a daemon for `port` inside an isolated `FF_RDP_HOME` and return
/// its PID (read from `daemon status`), or a message saying exactly how it
/// failed.
///
/// iter-246 Part A Theme D: this returned a bare `Option`, and its one
/// diagnostic printed the CLI's **stderr** — which is empty, because `ff-rdp`
/// writes its error envelopes to stdout. Iteration 211's second sweep recorded
/// the whole failure as:
///
/// ```text
/// autostart_daemon: eval failed:
/// e2e_sigterm_removes_registry: daemon never reported a pid
/// ```
///
/// — two lines that between them do not say whether the autostart lost a race,
/// whether the daemon came up and then went quiet, or whether the eval never
/// reached Firefox at all. The `Err` string now carries which of the three
/// happened, with both streams, so the next occurrence is triageable rather
/// than merely reproducible. (The iter-179 source scan cannot catch this shape:
/// it covers panic macros, and this was an `eprintln!`.)
fn autostart_daemon(home: &std::path::Path, port: u16) -> Result<u32, String> {
    // `eval` (unlike `tabs`, which connects to Firefox directly via
    // RdpConnection::connect and never touches resolve_connection_target)
    // routes through connect_tab.rs, so a call without --no-daemon genuinely
    // auto-starts the daemon. See the matching fix + note in
    // eval_object_leak_soak.rs.
    let init = Command::new(ff_rdp_bin())
        .env("FF_RDP_HOME", home)
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--timeout",
            "10000",
            "eval",
            "1",
        ])
        .output()
        .map_err(|e| format!("autostart_daemon: could not spawn `ff-rdp eval 1`: {e}"))?;
    if !init.status.success() {
        return Err(format!(
            "autostart_daemon: the autostart-triggering `eval 1` exited non-zero — {}",
            crate::common::output_note(&init)
        ));
    }

    // Poll daemon status for a pid.
    let started = std::time::Instant::now();
    let deadline = started + Duration::from_secs(10);
    let mut polls = 0usize;
    let mut last;
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
            .map_err(|e| {
                format!("autostart_daemon: could not spawn `ff-rdp daemon status`: {e}")
            })?;
        polls += 1;
        last = crate::common::output_note(&status);
        if status.status.success()
            && let Ok(json) = serde_json::from_slice::<serde_json::Value>(&status.stdout)
            && let Some(pid) = json["results"]["pid"]
                .as_u64()
                .and_then(|p| u32::try_from(p).ok())
        {
            return Ok(pid);
        }
        if std::time::Instant::now() >= deadline {
            return Err(format!(
                "autostart_daemon: `eval 1` succeeded, so the autostart was triggered, but \
                 `daemon status` never reported a pid within {:?} over {polls} poll(s); last \
                 status: {last}",
                started.elapsed()
            ));
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

    let ff = LiveFirefox::headless_on_random_port();

    let home = tempfile::tempdir().expect("tempdir for FF_RDP_HOME");

    let daemon_pid = autostart_daemon(home.path(), ff.port())
        .unwrap_or_else(|why| panic!("e2e_sigterm_removes_registry: {why}"));
    eprintln!("e2e_sigterm_removes_registry: daemon pid={daemon_pid}");

    let reg = registry_path(home.path(), ff.port());
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
