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

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Return the path to the compiled `ff-rdp` binary under test.
pub fn ff_rdp_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_ff-rdp"))
}

/// Env var [`LiveFirefox::try_launch`] sets on every `ff-rdp launch` it
/// spawns, naming the currently-running test (iter-151 Theme A).
///
/// Mirrors the product's private `crate::util::profile_dir::SPAWNING_TEST_ENV`
/// — duplicated, not imported, because this crate ships no `[lib]` target
/// for an integration-test binary to pull from (the same reason this file
/// already duplicates `OWNER_PID_MARKER`-shaped constants per test file
/// rather than sharing them with `src/`). Keep both in sync by hand.
pub const SPAWNING_TEST_ENV: &str = "FF_RDP_LIVE_TEST_NAME";

/// Best-effort name of the currently running `#[test]` function.
///
/// `cargo test`'s harness spawns every test body on its own thread named
/// after the test's fully-qualified path (so panics/backtraces can identify
/// it) — this reads that name back for [`SPAWNING_TEST_ENV`]. Falls back to
/// `"unknown"` when no thread name is set (e.g. code calling this outside
/// the test harness), which is still strictly more useful than no marker at
/// all.
fn current_test_name() -> String {
    std::thread::current()
        .name()
        .map_or_else(|| "unknown".to_owned(), str::to_owned)
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

/// Environment variable that overrides the bounded launch-wait timeout
/// ([`launch_wait_timeout`]). Value is whole seconds.
pub const LAUNCH_TIMEOUT_ENV: &str = "FF_RDP_LIVE_LAUNCH_TIMEOUT_SECS";

/// The bounded wait a live launcher applies before giving up on the Firefox
/// debugger port (iter-113 Theme A).
///
/// Defaults to 30 s — enough for a cold headless Firefox to open its port even
/// under parallel-test contention — but is overridable via
/// [`LAUNCH_TIMEOUT_ENV`] so the `launch_times_out_fast` harness test can force
/// a sub-second bound, and so a wedged CI runner fails fast instead of hanging
/// for the whole job timeout (the failure mode that turned ungated live tests
/// into 10-minute CI stalls in iter-112).
///
/// A non-numeric or empty override falls back to the 30 s default rather than
/// panicking — a malformed env var should not itself break the harness.
pub fn launch_wait_timeout() -> Duration {
    parse_launch_timeout(std::env::var(LAUNCH_TIMEOUT_ENV).ok().as_deref())
}

/// Pure parsing half of [`launch_wait_timeout`]: given the raw
/// [`LAUNCH_TIMEOUT_ENV`] value (or `None` if unset), returns the bound.
///
/// Split out so the parsing rules (missing/non-numeric ⇒ 30 s default;
/// numeric ⇒ that many seconds) are unit-testable without touching the
/// process-wide env var — reading/writing `LAUNCH_TIMEOUT_ENV` itself is
/// unsafe to do from a test that might run concurrently with a live suite
/// reading it on another thread (see `live_113_launch_timeout`'s module docs).
pub fn parse_launch_timeout(raw: Option<&str>) -> Duration {
    match raw {
        Some(v) => match v.trim().parse::<u64>() {
            Ok(secs) => Duration::from_secs(secs),
            Err(_) => Duration::from_secs(30),
        },
        None => Duration::from_secs(30),
    }
}

/// Wait for Firefox's remote-debugging port to accept a TCP connection, within
/// the bounded [`launch_wait_timeout`]. Panics with a diagnostic naming the
/// launcher `bin` and `port` if the port never opens (iter-113 Theme A).
///
/// The pre-iter-113 launchers *silently skipped* when the port never came up,
/// which — combined with a bare (ungated) `#[test]` — let an absent or wedged
/// Firefox burn the entire CI job budget before timing out. This helper turns
/// that into an immediate, self-describing failure: the message names the
/// binary path, the port waited on, and the bound, so CI logs point straight at
/// the cause instead of an opaque hang.
///
/// Live suites gate their own bodies on [`live_tests_enabled`] and return early
/// when Firefox is unavailable, so this only fires once a launch has actually
/// been attempted and the port genuinely failed to open in time.
///
/// Takes `timeout` explicitly (rather than reading [`LAUNCH_TIMEOUT_ENV`]
/// itself, i.e. callers pass [`launch_wait_timeout`]) so
/// `launch_times_out_fast` can force a sub-second bound without mutating the
/// process-wide `LAUNCH_TIMEOUT_ENV` env var — `cargo test-live` (unlike CI's
/// `--test-threads=1` live job) runs test binaries with multiple threads by
/// default, and that harness test is intentionally *ungated* (see
/// `// allow-ungated-live:` on it), so it can run concurrently with
/// `#[ignore]`-gated live suites that spawn real Firefox and read
/// [`launch_wait_timeout`] on another thread. `std::env::set_var` mutates
/// process-global state visible to every thread, so racing the two would risk
/// truncating an in-flight real launch's wait to the test's 1 s override.
/// Taking the bound as a parameter keeps the test hermetic.
pub fn wait_for_debugger_port_within(bin: &std::path::Path, port: u16, timeout: Duration) {
    if wait_for_tcp(port, timeout) {
        return;
    }
    panic!(
        "live launch timed out after {}s waiting for the Firefox remote-debugging \
         port {port} to open (launcher: {}). Set {LAUNCH_TIMEOUT_ENV} to change the \
         bound; a stuck port here usually means Firefox is absent or wedged.",
        timeout.as_secs(),
        bin.display(),
    );
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

/// RAII guard that kills a Firefox process by raw PID on drop.
///
/// Use this for instances this suite spawns *indirectly* — notably the fresh
/// Firefox that `ff-rdp launch --replace` starts after reaping the prior one.
/// A [`LiveFirefox`] guard only owns the PID it launched itself, so after a
/// `--replace` it reaps a process that is already dead while the replacement
/// survives the test. Bind this guard from the replacement's `results.pid`
/// *before* any assertion, so a panic still unwinds through the kill.
pub struct FirefoxGuard(u32);

impl FirefoxGuard {
    pub fn new(pid: u32) -> Self {
        Self(pid)
    }

    pub fn pid(&self) -> u32 {
        self.0
    }
}

impl Drop for FirefoxGuard {
    fn drop(&mut self) {
        kill_pid(self.0);
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

/// Env var overriding where [`record_live_launch`] appends its line.
pub const LIVE_LAUNCH_LOG_ENV: &str = "FF_RDP_LIVE_LAUNCH_LOG";

/// Append one line per successful live Firefox launch to a file that outlives
/// the test process (iter-158 Theme D).
///
/// This replaces `eprintln!("LiveFirefox: pid=… port=…")`, which was a
/// diagnostic only the *failing* path could ever show: libtest captures test
/// stderr and discards it for passing tests, so the line claiming to document
/// the passing path was invisible on exactly that path — zero occurrences in a
/// log of 170 passing tests.
///
/// Defaults to `target/live-launches.log` (next to the test binaries) so it
/// works with no configuration; [`LIVE_LAUNCH_LOG_ENV`] overrides the path.
/// Best-effort: a failure to write must never fail a test.
fn record_live_launch(pid: u32, port: u16) {
    let path = if let Some(p) = std::env::var_os(LIVE_LAUNCH_LOG_ENV) {
        PathBuf::from(p)
    } else {
        // CARGO_BIN_EXE_ff-rdp is `<target>/<profile>/ff-rdp`, so two levels
        // up is the target directory.
        let bin = ff_rdp_bin();
        let Some(target_dir) = bin.parent().and_then(std::path::Path::parent) else {
            return;
        };
        target_dir.join("live-launches.log")
    };
    // One `write_all` of a short line: concurrent appenders from separate test
    // binaries interleave whole lines rather than fragments.
    let line = format!(
        "{}\t{}\tpid={pid}\tport={port}\n",
        chrono_now_rfc3339(),
        current_test_name(),
    );
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = f.write_all(line.as_bytes());
    }
}

/// RFC-3339-ish timestamp without pulling `chrono` into the test binaries.
fn chrono_now_rfc3339() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    format!("epoch={secs}")
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

    /// Launch Firefox headless on a random port, **panicking** with a full
    /// diagnostic if it cannot be launched (iter-158 Theme D).
    ///
    /// Pre-iter-158 this returned `Option<Self>` and every one of the ~150 call
    /// sites did:
    ///
    /// ```ignore
    /// let Some(ff) = LiveFirefox::headless_on_random_port() else {
    ///     eprintln!("…: <the skip notice>");
    ///     return;                                  // ← libtest reports `ok`
    /// };
    /// ```
    ///
    /// libtest captures test stderr and discards it for passing tests, so a
    /// green run carried no evidence of how many of its `ok` results had
    /// reached Firefox at all — verified: **zero** `LiveFirefox: pid=` lines in
    /// a log containing 170 passing tests. That is
    /// `iteration-155-live-skip-reports-green`'s defect surviving through a
    /// second door: iter-155 made unmet *env gates* report `ignored`, while the
    /// larger fake-`ok` source (Firefox failed to launch) was untouched and
    /// still counted as `executed` by `live-sweep`.
    ///
    /// A test that genuinely tolerates an absent Firefox belongs behind an
    /// `#[ignore]` gate `live-sweep` already understands, not behind a runtime
    /// early return.
    pub fn headless_on_random_port() -> Self {
        Self::headless_on_random_port_with_args(&[]).0
    }

    /// Like [`LiveFirefox::headless_on_random_port`], but forwards `extra_args`
    /// to `ff-rdp launch` (e.g. `["--window-size", "390x844"]`) and also returns
    /// the parsed `launch` JSON envelope so the caller can inspect fields
    /// `ff-rdp launch` reports (e.g. `results.window_size`, `results.warnings`)
    /// that a later `eval` call can't recover after the fact.
    ///
    /// Panics on failure — see [`LiveFirefox::headless_on_random_port`].
    pub fn headless_on_random_port_with_args(extra_args: &[&str]) -> (Self, serde_json::Value) {
        match Self::try_headless_on_random_port_with_args(extra_args) {
            Ok(result) => result,
            Err(attempts) => panic!(
                "LiveFirefox: could not launch Firefox after {} attempt(s) \
                 (test: {}, launcher: {}, extra args: {:?}).\n{}",
                attempts.len(),
                current_test_name(),
                ff_rdp_bin().display(),
                extra_args,
                attempts.join("\n"),
            ),
        }
    }

    /// Fallible variant kept for the harness's own negative tests and used by
    /// the panicking wrappers above. `Err` carries one diagnostic string per
    /// attempt.
    pub fn try_headless_on_random_port() -> Option<Self> {
        Self::try_headless_on_random_port_with_args(&[])
            .ok()
            .map(|(ff, _)| ff)
    }

    /// Retry loop shared by the panicking and fallible entry points. Tries up
    /// to 3 ports to handle rare port-allocation collisions (common in CI with
    /// parallel test jobs).
    fn try_headless_on_random_port_with_args(
        extra_args: &[&str],
    ) -> Result<(Self, serde_json::Value), Vec<String>> {
        let mut failures = Vec::new();
        for attempt in 0..3u8 {
            match Self::try_launch(extra_args, attempt) {
                Ok(result) => return Ok(result),
                Err(diagnostic) => {
                    failures.push(diagnostic);
                    if attempt < 2 {
                        std::thread::sleep(Duration::from_millis(200));
                    }
                }
            }
        }
        Err(failures)
    }

    /// One launch attempt. `Err` carries everything the failure path knows:
    /// the attempt number, the port, the `ff-rdp launch` exit status, and its
    /// captured stdout **and** stderr.
    ///
    /// `stderr` used to be `Stdio::null()`, which discarded the product's own
    /// diagnostic before anything could read it — so even a caller that wanted
    /// to report the failure had nothing to report.
    fn try_launch(extra_args: &[&str], attempt: u8) -> Result<(Self, serde_json::Value), String> {
        let Some(port) = free_port() else {
            return Err(format!(
                "attempt {attempt}: could not bind 127.0.0.1:0 to discover a free port"
            ));
        };

        let output = Command::new(ff_rdp_bin())
            .args(["launch", "--headless", "--debug-port", &port.to_string()])
            .args(extra_args)
            // iter-151 Theme A: identify the spawning test so a leaked
            // profile is traceable from the artifact alone — see
            // `SPAWNING_TEST_ENV`'s doc comment.
            .env(SPAWNING_TEST_ENV, current_test_name())
            .output()
            .map_err(|e| {
                format!(
                    "attempt {attempt} (port {port}): could not spawn `{} launch`: {e}",
                    ff_rdp_bin().display()
                )
            })?;

        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let context = format!("attempt {attempt} (port {port}): exit {}", output.status)
            + &format!("\n  stdout: {stdout}\n  stderr: {stderr}");

        if !output.status.success() {
            return Err(format!("{context}\n  → `ff-rdp launch` returned non-zero"));
        }

        let json: serde_json::Value = serde_json::from_slice(&output.stdout)
            .map_err(|e| format!("{context}\n  → launch stdout is not JSON: {e}"))?;
        let firefox_pid = json["results"]["pid"]
            .as_u64()
            .and_then(|p| u32::try_from(p).ok())
            .ok_or_else(|| format!("{context}\n  → launch JSON has no numeric results.pid"))?;

        record_live_launch(firefox_pid, port);

        // Bounded, env-overridable wait (iter-113 Theme A) on top of the
        // product's own `--launch-timeout` bound (iter-158 Theme A).
        if !wait_for_tcp(port, launch_wait_timeout()) {
            kill_pid(firefox_pid);
            return Err(format!(
                "{context}\n  → launch reported pid {firefox_pid} but port {port} never \
                 accepted a connection within {}s ({LAUNCH_TIMEOUT_ENV})",
                launch_wait_timeout().as_secs()
            ));
        }

        let ff = Self { firefox_pid, port };

        // Wait until at least one tab is available.
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut last_tabs_error;
        loop {
            let out = Command::new(ff_rdp_bin())
                .args(base_args(ff.port))
                .arg("tabs")
                .output();
            match &out {
                Ok(o) if o.status.success() => {
                    let tab_count = serde_json::from_slice::<serde_json::Value>(&o.stdout)
                        .ok()
                        .and_then(|j| j["total"].as_u64())
                        .unwrap_or(0);
                    if tab_count >= 1 {
                        return Ok((ff, json));
                    }
                    last_tabs_error = format!("`tabs` reported total={tab_count}");
                }
                Ok(o) => {
                    last_tabs_error = format!(
                        "`tabs` exit {}: {}",
                        o.status,
                        String::from_utf8_lossy(&o.stderr).trim()
                    );
                }
                Err(e) => last_tabs_error = format!("`tabs` could not be spawned: {e}"),
            }
            if std::time::Instant::now() >= deadline {
                let pid = ff.firefox_pid;
                drop(ff);
                return Err(format!(
                    "{context}\n  → Firefox (pid {pid}) opened port {port} but exposed no tab \
                     within 10s: {last_tabs_error}"
                ));
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
    // Held so `Drop` can `wait()` after killing — without reaping, the
    // process would remain a zombie (Unix) until some other code waits on
    // it, since it is a direct child of this test process.
    child: std::process::Child,
}

impl RawFirefox {
    pub fn pid(&self) -> u32 {
        self.pid
    }
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Launch a headless Firefox directly on a free port with a throwaway
    /// profile.
    ///
    /// Panics if Firefox is unavailable or the debug port never comes up —
    /// same rationale as [`LiveFirefox::headless_on_random_port`] (iter-158
    /// Theme D): a silent `None` here turned an unrunnable kill-scoping test
    /// into a green one.
    pub fn headless_on_random_port() -> Self {
        match Self::try_headless_on_random_port() {
            Ok(ff) => ff,
            Err(diagnostic) => panic!("RawFirefox: {diagnostic}"),
        }
    }

    fn try_headless_on_random_port() -> Result<Self, String> {
        let firefox = find_firefox_binary().ok_or_else(|| {
            "Firefox binary not found on PATH or in a well-known location".to_owned()
        })?;
        let port = free_port()
            .ok_or_else(|| "could not bind 127.0.0.1:0 to discover a free port".to_owned())?;
        // A profile dir that is NOT under ff-rdp's managed root and does NOT
        // match the `ff-rdp-profile-*` convention.
        let profile = std::env::temp_dir().join(format!("raw-ff-{}-{port}", std::process::id()));
        std::fs::create_dir_all(&profile)
            .map_err(|e| format!("could not create profile {}: {e}", profile.display()))?;

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
        .map_err(|e| format!("could not write {}/user.js: {e}", profile.display()))?;

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
            .map_err(|e| format!("could not spawn {}: {e}", firefox.display()))?;
        let pid = child.id();

        let ff = Self {
            pid,
            port,
            profile,
            child,
        };
        // Bounded, env-overridable wait (iter-113 Theme A).
        if wait_for_tcp(port, launch_wait_timeout()) {
            Ok(ff)
        } else {
            let bound = launch_wait_timeout().as_secs();
            // `ff` drops here, killing the process and removing the profile.
            Err(format!(
                "{} (pid {pid}) never opened debug port {port} within {bound}s \
                 (raise {LAUNCH_TIMEOUT_ENV})",
                firefox.display()
            ))
        }
    }
}

impl Drop for RawFirefox {
    fn drop(&mut self) {
        // `kill_pid` signals by raw PID (mirrors `LiveFirefox`, and matches
        // what the kill-scoping guard under test actually does), then we
        // `wait()` on the held `Child` to reap it — otherwise a directly
        // spawned child left un-waited becomes a zombie on Unix.
        kill_pid(self.pid);
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.profile);
    }
}

// ---------------------------------------------------------------------------
// Canonical-color comparison (iter-114 Theme A)
// ---------------------------------------------------------------------------
//
// Firefox 152 started serializing some computed `color`/`background-color`
// values as CSS keywords (e.g. `red`) where older versions always returned
// `rgb(255, 0, 0)`. Tests that hard-coded the `rgb()` form broke on upgrade.
// `parse_css_color` normalizes keyword / `#rgb` / `#rrggbb` / `#rrggbbaa` /
// `#rgba` / `rgb(...)` / `rgba(...)` into one `(r, g, b, a)` tuple (alpha
// 0..=255) so assertions survive serialization drift in either direction.

/// An 8-bit RGBA color, used as the canonical form for [`parse_css_color`].
pub type Rgba = (u8, u8, u8, u8);

/// CSS keyword → RGB table, intentionally minimal: only the named colors
/// actually produced by this suite's fixtures, not all 148 CSS keywords.
const CSS_KEYWORDS: &[(&str, (u8, u8, u8))] = &[
    ("red", (255, 0, 0)),
    ("green", (0, 128, 0)),
    ("blue", (0, 0, 255)),
    ("white", (255, 255, 255)),
    ("black", (0, 0, 0)),
    ("yellow", (255, 255, 0)),
    ("transparent", (0, 0, 0)),
];

/// Parse a CSS color string (keyword, `#rgb`, `#rrggbb`, `#rgba`, `#rrggbbaa`,
/// `rgb(...)`, or `rgba(...)`) into a canonical [`Rgba`] tuple.
///
/// Returns `None` for unrecognized input (e.g. `currentcolor`, unsupported
/// keywords, or malformed syntax) — callers should treat that as "cannot
/// compare canonically" rather than "colors differ".
pub fn parse_css_color(s: &str) -> Option<Rgba> {
    let s = s.trim();

    if let Some(hex) = s.strip_prefix('#') {
        return parse_hex_color(hex);
    }

    if let Some(inner) = s.strip_prefix("rgba(").and_then(|r| r.strip_suffix(')')) {
        return parse_rgb_components(inner, true);
    }
    if let Some(inner) = s.strip_prefix("rgb(").and_then(|r| r.strip_suffix(')')) {
        return parse_rgb_components(inner, false);
    }

    let lower = s.to_ascii_lowercase();
    if lower == "transparent" {
        return Some((0, 0, 0, 0));
    }
    CSS_KEYWORDS
        .iter()
        .find(|(kw, _)| *kw == lower)
        .map(|(_, (r, g, b))| (*r, *g, *b, 255))
}

fn parse_hex_color(hex: &str) -> Option<Rgba> {
    let digit = |c: u8| -> Option<u8> {
        match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            b'A'..=b'F' => Some(c - b'A' + 10),
            _ => None,
        }
    };
    let pair = |hi: u8, lo: u8| -> Option<u8> { Some(digit(hi)? * 16 + digit(lo)?) };
    let nibble_dup = |c: u8| -> Option<u8> { Some(digit(c)? * 17) };

    let bytes = hex.as_bytes();
    match bytes.len() {
        3 => Some((
            nibble_dup(bytes[0])?,
            nibble_dup(bytes[1])?,
            nibble_dup(bytes[2])?,
            255,
        )),
        4 => Some((
            nibble_dup(bytes[0])?,
            nibble_dup(bytes[1])?,
            nibble_dup(bytes[2])?,
            nibble_dup(bytes[3])?,
        )),
        6 => Some((
            pair(bytes[0], bytes[1])?,
            pair(bytes[2], bytes[3])?,
            pair(bytes[4], bytes[5])?,
            255,
        )),
        8 => Some((
            pair(bytes[0], bytes[1])?,
            pair(bytes[2], bytes[3])?,
            pair(bytes[4], bytes[5])?,
            pair(bytes[6], bytes[7])?,
        )),
        _ => None,
    }
}

/// Round `v` (assumed already within `0.0..=255.0`) to the nearest `u8`.
///
/// Callers are expected to validate or clamp the input range beforehand —
/// this only performs the float-to-int conversion, isolated here so the
/// sign-loss/truncation lints are acknowledged in exactly one place instead
/// of at every call site.
fn round_to_u8(v: f64) -> u8 {
    #[allow(
        clippy::cast_sign_loss,
        clippy::cast_possible_truncation,
        reason = "v is always clamped/validated to 0.0..=255.0 by the caller before this call"
    )]
    let rounded = v.round() as u8;
    rounded
}

/// Parse the comma-separated inner content of `rgb(...)`/`rgba(...)`.
///
/// Accepts an integer or percentage alpha (`0.5` or `50%`) when
/// `has_alpha` is true; otherwise defaults alpha to 255 (fully opaque).
fn parse_rgb_components(inner: &str, has_alpha: bool) -> Option<Rgba> {
    let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
    let expected = if has_alpha { 4 } else { 3 };
    if parts.len() != expected {
        return None;
    }
    let parse_channel = |p: &str| -> Option<u8> {
        let v: f64 = p.parse().ok()?;
        if !(0.0..=255.0).contains(&v) {
            return None;
        }
        Some(round_to_u8(v))
    };
    let r = parse_channel(parts[0])?;
    let g = parse_channel(parts[1])?;
    let b = parse_channel(parts[2])?;
    let a = if has_alpha {
        let raw = parts[3];
        if let Some(pct) = raw.strip_suffix('%') {
            let v: f64 = pct.parse().ok()?;
            round_to_u8(v.clamp(0.0, 100.0) / 100.0 * 255.0)
        } else {
            let v: f64 = raw.parse().ok()?;
            round_to_u8(v.clamp(0.0, 1.0) * 255.0)
        }
    } else {
        255
    };
    Some((r, g, b, a))
}

/// Assert that two CSS color strings are equal under [`parse_css_color`]'s
/// canonical form, regardless of which literal syntax each uses.
///
/// Panics with both the original strings and their parsed forms if either
/// fails to parse, or if the parsed colors differ — the panic message is the
/// diagnostic a live-test failure needs, so callers don't have to build
/// their own.
pub fn assert_colors_equal(actual: &str, expected: &str, context: &str) {
    let a = parse_css_color(actual);
    let e = parse_css_color(expected);
    assert!(
        a.is_some() && e.is_some() && a == e,
        "{context}: color mismatch — actual={actual:?} (parsed={a:?}), \
         expected={expected:?} (parsed={e:?})"
    );
}

// ---------------------------------------------------------------------------
// PNG pixel decoding (iter-144 Theme D)
// ---------------------------------------------------------------------------
//
// `live_144_full_page_no_duplicate_header` needs actual pixel rows — the
// existing `png_dimensions`-style IHDR peek (used by earlier live suites)
// only reads the uncompressed header, not the (zlib-compressed) image data.
// The `png` crate is a dev-dependency solely for this.

/// A decoded RGBA8 raster: `width`/`height` in pixels, `pixels` row-major,
/// 4 bytes (R, G, B, A) per pixel.
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl DecodedImage {
    /// Return the `(r, g, b, a)` pixel at `(x, y)`, or `None` if out of
    /// bounds.
    pub fn pixel(&self, x: u32, y: u32) -> Option<(u8, u8, u8, u8)> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let idx = ((y * self.width + x) * 4) as usize;
        let px = self.pixels.get(idx..idx + 4)?;
        Some((px[0], px[1], px[2], px[3]))
    }

    /// Fraction (`0.0..=1.0`) of pixels in row `y` whose color is within
    /// `tolerance` (per channel, RGB only — alpha ignored) of `color`.
    /// Returns `0.0` for an out-of-bounds row.
    pub fn row_color_fraction(&self, y: u32, color: (u8, u8, u8), tolerance: u8) -> f64 {
        if y >= self.height || self.width == 0 {
            return 0.0;
        }
        let close = |a: u8, b: u8| a.abs_diff(b) <= tolerance;
        let mut matches = 0u32;
        for x in 0..self.width {
            if let Some((r, g, b, _)) = self.pixel(x, y)
                && close(r, color.0)
                && close(g, color.1)
                && close(b, color.2)
            {
                matches += 1;
            }
        }
        f64::from(matches) / f64::from(self.width)
    }

    /// Count the number of separate vertical runs of rows whose
    /// [`row_color_fraction`] for `color` meets or exceeds `min_fraction`.
    ///
    /// A page with one `position: fixed`/`sticky` header of `color` produces
    /// exactly one run; the iter-144 Theme D duplicate-header artifact
    /// produces more than one (the header repainted at an internal tile
    /// boundary further down the full-page capture).
    pub fn color_row_run_count(
        &self,
        color: (u8, u8, u8),
        tolerance: u8,
        min_fraction: f64,
    ) -> u32 {
        let mut runs = 0u32;
        let mut in_run = false;
        for y in 0..self.height {
            let matches = self.row_color_fraction(y, color, tolerance) >= min_fraction;
            if matches && !in_run {
                runs += 1;
            }
            in_run = matches;
        }
        runs
    }
}

/// Decode PNG bytes (as produced by `ff-rdp screenshot --base64`) into a
/// [`DecodedImage`]. Panics on malformed input — live tests want a loud
/// failure naming the decode error, not a silent skip, since a decode
/// failure here means the screenshot command itself produced a broken PNG.
pub fn decode_png(bytes: &[u8]) -> DecodedImage {
    let decoder = png::Decoder::new(bytes);
    let mut reader = decoder
        .read_info()
        .unwrap_or_else(|e| panic!("decode_png: failed to read PNG header: {e}"));
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut buf)
        .unwrap_or_else(|e| panic!("decode_png: failed to decode PNG frame: {e}"));
    buf.truncate(info.buffer_size());

    let width = info.width;
    let height = info.height;

    // Normalize to RGBA8 regardless of the PNG's actual color type — the
    // captures under test are always RGB(A)8, but this keeps the helper
    // honest about what it assumes rather than silently misreading bytes.
    let pixels = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => {
            let mut out = Vec::with_capacity(buf.len() / 3 * 4);
            for chunk in buf.chunks_exact(3) {
                out.extend_from_slice(chunk);
                out.push(255);
            }
            out
        }
        other => panic!("decode_png: unsupported color type {other:?} (expected Rgb/Rgba)"),
    };

    DecodedImage {
        width,
        height,
        pixels,
    }
}

#[cfg(test)]
mod color_tests {
    use super::*;

    #[test]
    fn keyword_matches_rgb_both_directions() {
        assert_eq!(parse_css_color("red"), parse_css_color("rgb(255, 0, 0)"));
        assert_eq!(parse_css_color("rgb(255, 0, 0)"), parse_css_color("red"));
        assert_eq!(parse_css_color("blue"), parse_css_color("rgb(0, 0, 255)"));
        assert_eq!(parse_css_color("rgb(0, 0, 255)"), parse_css_color("blue"));
    }

    #[test]
    fn hex_matches_rgb_both_directions() {
        assert_eq!(
            parse_css_color("#ff0000"),
            parse_css_color("rgb(255, 0, 0)")
        );
        assert_eq!(
            parse_css_color("rgb(255, 0, 0)"),
            parse_css_color("#ff0000")
        );
        assert_eq!(parse_css_color("#f00"), parse_css_color("rgb(255, 0, 0)"));
        assert_eq!(
            parse_css_color("rgb(0, 128, 0)"),
            parse_css_color("#008000")
        );
    }

    #[test]
    fn keyword_matches_hex_both_directions() {
        assert_eq!(parse_css_color("red"), parse_css_color("#ff0000"));
        assert_eq!(parse_css_color("#ff0000"), parse_css_color("red"));
        assert_eq!(parse_css_color("white"), parse_css_color("#fff"));
        assert_eq!(parse_css_color("#fff"), parse_css_color("white"));
    }

    #[test]
    fn rgba_alpha_channel_parses() {
        assert_eq!(
            parse_css_color("rgba(255, 0, 0, 1)"),
            Some((255, 0, 0, 255))
        );
        assert_eq!(parse_css_color("rgba(255, 0, 0, 0)"), Some((255, 0, 0, 0)));
        assert_eq!(parse_css_color("rgba(0, 0, 0, 0.5)"), Some((0, 0, 0, 128)));
    }

    #[test]
    fn eight_digit_hex_matches_rgba() {
        assert_eq!(
            parse_css_color("#ff000080"),
            parse_css_color("rgba(255, 0, 0, 0.5)")
        );
    }

    #[test]
    fn unrecognized_input_returns_none() {
        assert_eq!(parse_css_color("currentcolor"), None);
        assert_eq!(parse_css_color("not-a-color"), None);
    }

    #[test]
    fn assert_colors_equal_passes_for_equivalent_forms() {
        assert_colors_equal("red", "rgb(255, 0, 0)", "test context");
        assert_colors_equal("rgb(0, 0, 255)", "blue", "test context");
    }

    #[test]
    #[should_panic(expected = "color mismatch")]
    fn assert_colors_equal_panics_for_different_colors() {
        assert_colors_equal("red", "blue", "test context");
    }
}

// ---------------------------------------------------------------------------
// Self-hosted fixture HTTP server (iter-114 Theme C)
// ---------------------------------------------------------------------------

/// One route served by [`FixtureServer`]: response `Content-Type`, body, and
/// any extra response headers.
#[derive(Clone, Default)]
pub struct FixtureRoute {
    pub content_type: &'static str,
    pub body: Vec<u8>,
    /// Additional `Name: value` response headers, e.g. `Set-Cookie`. Empty by
    /// default so existing struct-literal and `html()` construction sites are
    /// unaffected — use [`FixtureRoute::with_header`] to add one.
    pub extra_headers: Vec<(String, String)>,
}

impl FixtureRoute {
    /// Convenience constructor for `text/html; charset=utf-8` routes, which
    /// is what every current fixture-server consumer serves.
    pub fn html(body: impl Into<String>) -> Self {
        Self {
            content_type: "text/html; charset=utf-8",
            body: body.into().into_bytes(),
            extra_headers: Vec::new(),
        }
    }

    /// Builder method: append an extra `name: value` response header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.extra_headers.push((name.into(), value.into()));
        self
    }
}

/// A minimal, std-only, single-threaded static HTTP server for live tests
/// that need to crawl a small set of interlinked local pages instead of
/// depending on a real network origin.
///
/// Binds `127.0.0.1:0` (an ephemeral port — never fixed, so parallel test
/// runs never collide), serves an in-source route map passed by the caller,
/// and shuts down cleanly on `Drop`: a shutdown flag is set, then a final
/// connection is made to the listener to unblock `accept()` so the
/// background thread can observe the flag and exit instead of blocking
/// forever on the next `incoming()` call.
///
/// Every response is `Connection: close` — no keep-alive — matching the
/// existing single-shot servers (`spawn_html_server`, `spawn_fixture_server`)
/// this is modeled on and could later replace.
pub struct FixtureServer {
    port: u16,
    shutdown: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl FixtureServer {
    /// Start serving `routes` (request path → response) on an ephemeral
    /// localhost port. Unknown paths get a `404`. Returns `None` if the
    /// ephemeral port cannot be bound.
    pub fn start(routes: HashMap<String, FixtureRoute>) -> Option<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").ok()?;
        let port = listener.local_addr().ok()?.port();
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_for_thread = Arc::clone(&shutdown);

        let handle = std::thread::spawn(move || {
            for stream in listener.incoming() {
                if shutdown_for_thread.load(Ordering::Acquire) {
                    break;
                }
                let Ok(stream) = stream else { continue };
                handle_connection(stream, &routes);
            }
        });

        Some(Self {
            port,
            shutdown,
            handle: Some(handle),
        })
    }

    /// The `http://127.0.0.1:<port>` base URL routes are relative to.
    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    pub fn port(&self) -> u16 {
        self.port
    }
}

impl Drop for FixtureServer {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
        // Unblock the background thread's `accept()` with a dummy connection
        // so it observes the shutdown flag and exits instead of hanging until
        // process teardown.
        if let Ok(mut stream) = TcpStream::connect(("127.0.0.1", self.port)) {
            let _ = stream.write_all(b"GET / HTTP/1.1\r\nConnection: close\r\n\r\n");
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// Read one HTTP request line off `stream`, look up its path in `routes`,
/// and write back the matching response (or a `404`).
fn handle_connection(mut stream: TcpStream, routes: &HashMap<String, FixtureRoute>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));

    let mut buf = [0u8; 4096];
    let Ok(n) = stream.read(&mut buf) else {
        return;
    };
    let request = String::from_utf8_lossy(&buf[..n]);
    let Some(request_line) = request.lines().next() else {
        return;
    };
    // Request line form: "GET /path HTTP/1.1"
    let path = request_line
        .split_whitespace()
        .nth(1)
        .unwrap_or("/")
        .split('?')
        .next()
        .unwrap_or("/");

    if let Some(route) = routes.get(path) {
        let mut header = format!(
            "HTTP/1.1 200 OK\r\n\
             Content-Type: {}\r\n\
             Content-Length: {}\r\n\
             Cache-Control: no-store\r\n",
            route.content_type,
            route.body.len()
        );
        for (name, value) in &route.extra_headers {
            header.push_str(name);
            header.push_str(": ");
            header.push_str(value);
            header.push_str("\r\n");
        }
        header.push_str("Connection: close\r\n\r\n");
        let _ = stream.write_all(header.as_bytes());
        let _ = stream.write_all(&route.body);
    } else {
        let body = b"Not Found";
        let header = format!(
            "HTTP/1.1 404 Not Found\r\n\
             Content-Type: text/plain\r\n\
             Content-Length: {}\r\n\
             Connection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(header.as_bytes());
        let _ = stream.write_all(body);
    }
}
