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
// iter-105 Theme D: process-cleanup helpers here call `libc::kill` via FFI.
// The crate default is `unsafe_code = "deny"`; this file-scoped allowance keeps
// the `// SAFETY:`-documented test helpers compiling wherever this module is
// `#[path]`-included, while production code stays denied.
#![allow(unsafe_code)]

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

/// Return `true` if a process with `pid` is currently alive.
///
/// Mirrors the product's `daemon::process::is_process_alive` (unreachable from
/// an integration-test binary) so the iter-110 Theme A0 kill-scoping test can
/// assert a foreign browser survives an `ff-rdp launch --replace`.
#[cfg(unix)]
pub fn pid_alive(pid: u32) -> bool {
    // SAFETY: `kill(pid, 0)` delivers no signal — it only probes existence.
    // Returns 0 if the process exists (and we may signal it), or -1 with ESRCH
    // when it does not. Any non-ESRCH error (e.g. EPERM) still means it exists.
    let rc = unsafe { libc::kill(pid.cast_signed(), 0) };
    if rc == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

/// Return `true` if a process with `pid` is currently alive (Windows).
#[cfg(windows)]
pub fn pid_alive(pid: u32) -> bool {
    unsafe {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        // SAFETY: OpenProcess returns NULL when the PID is invalid/dead, which
        // we check before closing the handle.
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            false
        } else {
            CloseHandle(h);
            true
        }
    }
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

/// Resolve the Firefox binary the same way the product's `commands::launch`
/// does, so [`RawFirefox`] can spawn Firefox *directly* without going through
/// `ff-rdp launch` (and therefore without planting an owner-PID marker).
///
/// Checks the same macOS/Windows well-known paths, then falls back to
/// `which`/`where`. Returns `None` if Firefox is not installed.
pub fn find_firefox_binary() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    let well_known: &[&str] = &[
        "/Applications/Firefox.app/Contents/MacOS/firefox",
        "/Applications/Firefox Developer Edition.app/Contents/MacOS/firefox",
        "/Applications/Firefox Nightly.app/Contents/MacOS/firefox",
    ];
    #[cfg(target_os = "windows")]
    let well_known: &[&str] = &[
        r"C:\Program Files\Mozilla Firefox\firefox.exe",
        r"C:\Program Files (x86)\Mozilla Firefox\firefox.exe",
    ];
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let well_known: &[&str] = &[];

    for p in well_known {
        let path = PathBuf::from(p);
        if path.is_file() {
            return Some(path);
        }
    }

    let (which_cmd, candidates): (&str, &[&str]) = if cfg!(target_os = "windows") {
        ("where", &["firefox.exe"])
    } else {
        (
            "which",
            &["firefox", "firefox-esr", "firefox-developer-edition"],
        )
    };
    for candidate in candidates {
        if let Ok(out) = Command::new(which_cmd).arg(candidate).output()
            && out.status.success()
        {
            let line = String::from_utf8_lossy(&out.stdout);
            if let Some(first) = line.lines().next() {
                let path = PathBuf::from(first.trim());
                if path.is_file() {
                    return Some(path);
                }
            }
        }
    }
    None
}

/// A Firefox instance launched **directly** (bypassing `ff-rdp launch`), so it
/// carries **no** owner-PID marker under ff-rdp's managed profile root.
///
/// This models a browser the *user* started by hand — the class of process the
/// iter-110 Theme A0 kill-scoping guard must never signal. Uses a throwaway
/// `-profile` dir well outside ff-rdp's managed root so nothing about it looks
/// ff-rdp-owned. `Drop` kills it and removes the temp profile.
pub struct RawFirefox {
    pid: u32,
    port: u16,
    profile: PathBuf,
}

impl RawFirefox {
    pub fn pid(&self) -> u32 {
        self.pid
    }
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Launch a headless Firefox directly on a free port with a throwaway
    /// profile. Returns `None` if Firefox is unavailable or the debug port
    /// never comes up.
    pub fn headless_on_random_port() -> Option<Self> {
        let firefox = find_firefox_binary()?;
        let port = free_port()?;
        // A profile dir that is NOT under ff-rdp's managed root and does NOT
        // match the `ff-rdp-profile-*` convention.
        let profile = std::env::temp_dir().join(format!("raw-ff-{}-{port}", std::process::id()));
        std::fs::create_dir_all(&profile).ok()?;

        // Firefox reads prefs at startup, so the debugger prefs MUST be on disk
        // before spawn — otherwise the --start-debugger-server port never opens
        // on a fresh profile.
        std::fs::write(
            profile.join("user.js"),
            "user_pref(\"devtools.debugger.remote-enabled\", true);\n\
             user_pref(\"devtools.chrome.enabled\", true);\n\
             user_pref(\"devtools.debugger.prompt-connection\", false);\n\
             user_pref(\"remote.prefs.recommended\", true);\n",
        )
        .ok()?;

        let child = Command::new(&firefox)
            .args([
                "-no-remote",
                "-headless",
                "-profile",
                &profile.to_string_lossy(),
                "--start-debugger-server",
                &port.to_string(),
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .ok()?;
        let pid = child.id();
        // Detach: we track the PID and kill it ourselves in Drop.
        std::mem::forget(child);

        let ff = Self { pid, port, profile };
        if wait_for_tcp(port, Duration::from_secs(30)) {
            Some(ff)
        } else {
            None // Drop cleans up
        }
    }
}

impl Drop for RawFirefox {
    fn drop(&mut self) {
        kill_pid(self.pid);
        let _ = std::fs::remove_dir_all(&self.profile);
    }
}
