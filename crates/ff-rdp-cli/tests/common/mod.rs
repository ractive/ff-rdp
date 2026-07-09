//! Shared live-test helpers for `ff-rdp-cli` integration tests.
//!
//! Since iter-100b the live tests are consolidated into a single `tests/live/`
//! target: `main.rs` declares this module once via
//! `#[path = "../common/mod.rs"] mod common;` and each suite refers to it as
//! `use crate::common::…`. (The other top-level test binaries still include it
//! per-file with `#[path = "common/mod.rs"] mod common;`.)
//!
//! All items carry `#[allow(dead_code)]` because not every binary uses every
//! helper — the same pattern used in `ff-rdp-core/tests/support/mod.rs`.

#![allow(dead_code)]

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

/// Return the path to the compiled `ff-rdp` binary under test.
pub fn ff_rdp_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_ff-rdp"))
}

/// True when live Firefox tests are enabled (`FF_RDP_LIVE_TESTS=1`).
///
/// Deduped in iter-100b from ~16 byte-identical copies that each live suite
/// used to define locally. The single divergent copy (`live_bulk_cap`, which
/// accepts any non-empty non-`0` value) intentionally keeps its own local
/// definition to preserve exact behavior.
pub fn live_tests_enabled() -> bool {
    std::env::var("FF_RDP_LIVE_TESTS").as_deref() == Ok("1")
}

/// True when live tests that make real network requests are enabled
/// (`FF_RDP_LIVE_NETWORK_TESTS=1`).
///
/// Deduped in iter-100b from 10 byte-identical local copies.
pub fn live_network_tests_enabled() -> bool {
    std::env::var("FF_RDP_LIVE_NETWORK_TESTS").as_deref() == Ok("1")
}

/// Build the common CLI arguments that point at a specific Firefox RDP port
/// with `--no-daemon` so tests don't accidentally spin up a background daemon.
pub fn base_args(port: u16) -> Vec<String> {
    vec![
        "--host".to_owned(),
        "127.0.0.1".to_owned(),
        "--port".to_owned(),
        port.to_string(),
        "--no-daemon".to_owned(),
    ]
}

/// Attempt to bind `:0` to discover a free port.
fn free_port() -> Option<u16> {
    let l = std::net::TcpListener::bind("127.0.0.1:0").ok()?;
    Some(l.local_addr().ok()?.port())
}

/// Poll until `127.0.0.1:port` accepts a TCP connection or `timeout` elapses.
fn wait_for_tcp(port: u16, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if std::net::TcpStream::connect(format!("127.0.0.1:{port}")).is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

/// Kill a process by PID, ignoring errors (process may already be gone).
#[cfg(unix)]
pub fn kill_pid(pid: u32) {
    unsafe {
        // SAFETY: kill(2) is safe to call with a valid PID and signal; ESRCH is
        // returned when the process no longer exists, which we intentionally ignore.
        libc::kill(pid.cast_signed(), libc::SIGKILL);
    }
}

/// Kill a process by PID, ignoring errors (process may already be gone).
#[cfg(windows)]
pub fn kill_pid(pid: u32) {
    unsafe {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_TERMINATE, TerminateProcess,
        };
        // SAFETY: OpenProcess returns NULL on failure, which we check; TerminateProcess
        // and CloseHandle are safe to call on a valid handle.
        let h = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if !h.is_null() {
            TerminateProcess(h, 1);
            CloseHandle(h);
        }
    }
}

/// A live Firefox instance launched via `ff-rdp launch --headless`.
///
/// Holds the Firefox PID and the RDP debug port.  `Drop` kills Firefox; the
/// temporary profile created by `ff-rdp launch` is left for the OS to reap
/// (deferred to a future cleanup pass — see iter-61o notes).
pub struct LiveFirefox {
    firefox_pid: u32,
    port: u16,
}

impl LiveFirefox {
    /// Return the RDP debug port Firefox is listening on.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Return the PID of the launched Firefox process.
    pub fn pid(&self) -> u32 {
        self.firefox_pid
    }

    /// Launch Firefox headless on a random port.
    ///
    /// Tries up to 3 ports to handle rare port-allocation collisions (common in
    /// CI with parallel test jobs).  Returns `None` if Firefox is unavailable or
    /// fails to become ready within 30 s.
    pub fn headless_on_random_port() -> Option<Self> {
        for attempt in 0..3u8 {
            match Self::try_launch() {
                Some(ff) => return Some(ff),
                None => {
                    if attempt < 2 {
                        std::thread::sleep(Duration::from_millis(200));
                    }
                }
            }
        }
        None
    }

    fn try_launch() -> Option<Self> {
        let port = free_port()?;

        let output = Command::new(ff_rdp_bin())
            .args(["launch", "--headless", "--debug-port", &port.to_string()])
            .stderr(std::process::Stdio::null())
            .output()
            .ok()?;

        if !output.status.success() {
            return None;
        }

        let json: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
        let firefox_pid = u32::try_from(json["results"]["pid"].as_u64()?).ok()?;

        eprintln!("LiveFirefox: pid={firefox_pid} port={port}");

        if !wait_for_tcp(port, Duration::from_secs(30)) {
            kill_pid(firefox_pid);
            return None;
        }

        let ff = Self { firefox_pid, port };

        // Wait until at least one tab is available.
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            let out = Command::new(ff_rdp_bin())
                .args(base_args(ff.port))
                .arg("tabs")
                .output();
            if let Ok(o) = out
                && o.status.success()
            {
                let tab_count = serde_json::from_slice::<serde_json::Value>(&o.stdout)
                    .ok()
                    .and_then(|j| j["total"].as_u64())
                    .unwrap_or(0);
                if tab_count >= 1 {
                    return Some(ff);
                }
            }
            if std::time::Instant::now() >= deadline {
                kill_pid(ff.firefox_pid);
                return None;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    /// Start the daemon for this Firefox instance and return its proxy port.
    ///
    /// Mirrors the `start_daemon_for` logic from `live_daemon_watch_targets.rs`.
    /// Returns `None` if the daemon doesn't start within a reasonable timeout.
    pub fn with_daemon(&self) -> Option<u16> {
        // Trigger daemon startup: an `eval` call without --no-daemon causes
        // auto-start. `tabs` does NOT work here — `tabs.rs` connects to
        // Firefox directly via `RdpConnection::connect` and never goes
        // through `resolve_connection_target`, so it never actually starts a
        // daemon (see the fix + note in `eval_object_leak_soak.rs`).
        let out = Command::new(ff_rdp_bin())
            .args([
                "--host",
                "127.0.0.1",
                "--port",
                &self.port.to_string(),
                "--timeout",
                "5000",
                "eval",
                "1",
            ])
            .output()
            .ok()?;

        if !out.status.success() {
            return None;
        }

        // Give the daemon a moment to write its registry.
        std::thread::sleep(Duration::from_millis(500));

        // Confirm daemon is running and return its proxy port.
        let status = Command::new(ff_rdp_bin())
            .args([
                "--host",
                "127.0.0.1",
                "--port",
                &self.port.to_string(),
                "daemon",
                "status",
            ])
            .output()
            .ok()?;

        let status_json = serde_json::from_slice::<serde_json::Value>(&status.stdout).ok()?;
        if status_json["results"]["running"].as_bool() != Some(true) {
            return None;
        }
        let daemon_port = status_json["results"]["port"]
            .as_u64()
            .and_then(|p| u16::try_from(p).ok())?;

        eprintln!("LiveFirefox: daemon proxy port={daemon_port}");
        Some(daemon_port)
    }
}

impl Drop for LiveFirefox {
    fn drop(&mut self) {
        kill_pid(self.firefox_pid);
    }
}
