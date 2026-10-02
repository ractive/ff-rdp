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
use std::io::{Read, Seek, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Output, Stdio};
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
pub fn current_test_name() -> String {
    std::thread::current()
        .name()
        .map_or_else(|| "unknown".to_owned(), str::to_owned)
}

/// A [`Command`] for the `ff-rdp` binary, pre-tagged with
/// [`SPAWNING_TEST_ENV`] so any managed profile the launch creates records
/// which test asked for it (iter-171).
///
/// **Use this for every `ff-rdp launch` a live test spawns directly.** Only
/// `LiveFirefox` set the env var before iter-171, so the ~20 suites that call
/// `ff-rdp launch` through a bare `Command` produced profiles whose owner-test
/// marker was simply never requested. That is why the four orphans the
/// iteration-168 postmortem chased all read `spawned by unknown test` — not,
/// as first assumed, because the marker failed to survive a kill.
///
/// The name is read off the current thread, so call this **on the test's own
/// thread**. A worker thread spawned by the test has no name and would tag the
/// profile `unknown`; those callers want
/// [`ff_rdp_launch_command_for`] with a name captured beforehand.
pub fn ff_rdp_launch_command() -> Command {
    ff_rdp_launch_command_for(&current_test_name())
}

/// [`ff_rdp_launch_command`] with an explicit owner name, for launches issued
/// from a worker thread (whose thread name is not the test's).
pub fn ff_rdp_launch_command_for(test_name: &str) -> Command {
    let mut cmd = Command::new(ff_rdp_bin());
    cmd.env(SPAWNING_TEST_ENV, test_name);
    cmd
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

/// True when tests that drive real third-party sites (BBC, Guardian, HN,
/// Wikipedia, MDN, …) are enabled (`FF_RDP_LIVE_SITES_TESTS=1`), in addition
/// to the live and network gates.
///
/// Only the weekly `sites` job in `.github/workflows/live.yml` sets it: those
/// pages change and go down on their own schedule, so they are a canary, never
/// part of the nightly sweep (2026-10-02 reset, `kb/research/step-back-2026-10-02.md` §5).
pub fn live_sites_tests_enabled() -> bool {
    live_tests_enabled()
        && live_network_tests_enabled()
        && std::env::var("FF_RDP_LIVE_SITES_TESTS").as_deref() == Ok("1")
}

/// Build the common CLI arguments that point at a specific Firefox RDP port.
pub fn base_args(port: u16) -> Vec<String> {
    vec![
        "--host".to_owned(),
        "127.0.0.1".to_owned(),
        "--port".to_owned(),
        port.to_string(),
    ]
}

/// Poll until nothing accepts connections on `127.0.0.1:port`, or `timeout`.
fn wait_for_tcp_closed(port: u16, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if TcpStream::connect(("127.0.0.1", port)).is_err() {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Attempt to bind `:0` to discover a free port.
fn free_port() -> Option<u16> {
    let l = std::net::TcpListener::bind("127.0.0.1:0").ok()?;
    Some(l.local_addr().ok()?.port())
}

/// Poll until `127.0.0.1:port` accepts a TCP connection or `timeout` elapses.
fn wait_for_tcp(port: u16, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    let address = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    while std::time::Instant::now() < deadline {
        // A bound, non-listening socket refuses immediately on Linux, but
        // macOS may silently drop its SYN. Keep each real connect probe inside
        // the launch wait's budget instead of inheriting the OS retry timeout.
        let probe_timeout = deadline
            .saturating_duration_since(std::time::Instant::now())
            .min(Duration::from_millis(100));
        if std::net::TcpStream::connect_timeout(&address, probe_timeout).is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

/// Environment variable that overrides the bounded launch-wait timeout
/// ([`launch_wait_timeout`]). Value is whole seconds.
pub const LAUNCH_TIMEOUT_ENV: &str = "FF_RDP_LIVE_LAUNCH_TIMEOUT_SECS";

/// Product-side launch timeout understood by `ff-rdp launch`.
///
/// The isolated session reads this separately from [`LAUNCH_TIMEOUT_ENV`]:
/// the product bound covers the launch command itself, while the live-harness
/// bound covers the post-receipt debugger-port check.
const PRODUCT_LAUNCH_TIMEOUT_ENV: &str = "FF_RDP_LAUNCH_TIMEOUT_SECS";

const DEFAULT_PRODUCT_LAUNCH_TIMEOUT_SECS: u64 = 30;
const PRODUCT_LAUNCH_STARTUP_GRACE: Duration = Duration::from_millis(500);

/// Resolve the product-side `launch --launch-timeout` value without mutating
/// process-global environment state.
///
/// This mirrors the product's flag/env resolver: missing, empty and malformed
/// values use 30 seconds, while a numeric value (including zero) is accepted.
pub(crate) fn parse_product_launch_timeout(raw: Option<&str>) -> Duration {
    match raw.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => value.parse::<u64>().map_or(
            Duration::from_secs(DEFAULT_PRODUCT_LAUNCH_TIMEOUT_SECS),
            Duration::from_secs,
        ),
        None => Duration::from_secs(DEFAULT_PRODUCT_LAUNCH_TIMEOUT_SECS),
    }
}

fn product_launch_timeout() -> Duration {
    parse_product_launch_timeout(std::env::var(PRODUCT_LAUNCH_TIMEOUT_ENV).ok().as_deref())
}

/// Bound the outer launch runner above the product's own work.
///
/// `ff-rdp launch` first spends 500 ms detecting an immediately-crashing
/// Firefox, then waits for the configured debugger-port budget. On failure it
/// kills and reaps that direct child and removes the managed profile. Give that
/// cleanup at least the harness's measured 5-second process-disappearance
/// allowance; an explicit larger kill-wait override raises this bound too.
pub(crate) fn isolated_launch_command_timeout(
    product_timeout: Duration,
    cleanup_allowance: Duration,
) -> Duration {
    product_timeout
        .saturating_add(PRODUCT_LAUNCH_STARTUP_GRACE)
        .saturating_add(cleanup_allowance)
}

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
/// Mirrors the product's `util::process::is_process_alive` (unreachable from
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
pub struct FirefoxGuard(Option<u32>);

impl FirefoxGuard {
    pub fn new(pid: u32) -> Self {
        Self(Some(pid))
    }

    /// The PID this guard owns, or `0` once [`disarm`](Self::disarm)ed.
    pub fn pid(&self) -> u32 {
        self.0.unwrap_or(0)
    }

    /// Give up ownership: `Drop` will not signal this PID (iter-242 Theme D).
    ///
    /// For the paths that have already *asserted* the process is gone — a
    /// `launch --replace` whose
    /// replacement was verified to carry a different PID. Keeping the guard
    /// live past that point means `Drop` unconditionally signals a PID the
    /// test knows is dead, and [`kill_pid`] does no ownership check, so at
    /// test scope it reintroduces exactly the recycled-PID hazard iter-110
    /// guards against in production: between the reap and the drop the OS may
    /// have reissued the number to something else. Iteration 151 removed the
    /// `ManuallyDrop` that was incidentally preventing this.
    ///
    /// `Drop` also skips a PID that is simply no longer alive, which covers
    /// the paths that never call this. `disarm` is the stronger statement —
    /// "this guard is finished" — and does not depend on winning a race
    /// against PID reuse to be correct.
    pub fn disarm(mut self) -> u32 {
        self.0.take().unwrap_or(0)
    }
}

impl Drop for FirefoxGuard {
    fn drop(&mut self) {
        let Some(pid) = self.0 else {
            return;
        };
        // iter-242 Theme D: never signal a PID that is already gone. `kill_pid`
        // checks neither liveness nor ownership, so signalling a reaped PID is
        // a signal to whatever the OS handed the number to next.
        if !pid_alive(pid) {
            return;
        }
        // iter-168: same bounded wait as `LiveFirefox::drop`. The process this
        // guard owns was started by `ff-rdp launch --replace`, so it carries an
        // owner-PID marker too and leaks the identical liveness window.
        kill_pid_and_wait(pid);
    }
}

/// Bind an RAII owner to the Firefox an `ff-rdp launch` started, straight from
/// its captured output (iter-242 Part B, Themes A and B).
///
/// Closes the spawn→guard window by construction: the PID is extracted and the
/// guard is built in one step, with no fallible parsing or assertion in
/// between. The shape it replaces —
///
/// ```ignore
/// let json: Value = serde_json::from_slice(&out.stdout).ok()?;   // may return
/// let pid = json["results"]["pid"].as_u64()?;                     // may return
/// let guard = FirefoxGuard::new(pid);                             // …too late
/// ```
///
/// drops a launched Firefox on the floor whenever a parse misses, and nothing
/// reaps it.
///
/// `None` means no Firefox to own: the launch exited non-zero, or its stdout
/// carried no `results.pid`. Callers that *require* a launch to have succeeded
/// should assert on `out.status` themselves — and must do it **after** calling
/// this, so the regression path where the launch unexpectedly succeeds is
/// still owned.
pub fn guard_launched_firefox(out: &std::process::Output) -> Option<FirefoxGuard> {
    if !out.status.success() {
        return None;
    }
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    let pid = u32::try_from(json["results"]["pid"].as_u64()?).ok()?;
    Some(FirefoxGuard::new(pid))
}

/// What the live document reported when a readiness poll gave up (iter-242
/// Part C).
///
/// Exists so a test asserting on a third-party page's content can *name* why
/// it failed instead of only reporting the value it did not get. The two
/// diagnoses the iteration-175 sweep could not tell apart were "the document
/// was not there yet" (our readiness contract) and "the site answered with
/// something else" (theirs), and `document.title == ""` is consistent with
/// both.
#[derive(Debug, Clone)]
pub struct DocumentState {
    pub ready_state: String,
    pub title: String,
    pub href: String,
    pub waited: Duration,
}

impl DocumentState {
    /// A one-line diagnosis naming which side the failure is on.
    ///
    /// `readyState` is the discriminator: a document that never completes is a
    /// readiness/transport problem, while one that completes with the wrong
    /// title is the site having answered differently — a rate-limit
    /// interstitial, a redesign, an outage page. Neither is a verdict on which
    /// side should *change*; it is the evidence that decision needs, which
    /// asserting on the title alone never produced.
    pub fn diagnosis(&self, expected: &str) -> String {
        if self.ready_state != "complete" {
            format!(
                "READINESS: document.readyState was {:?} (never \"complete\") after {:?} at \
                 {} — navigate reported success, so this is our readiness contract or the \
                 transport, not the page's content",
                self.ready_state, self.waited, self.href
            )
        } else if self.title.is_empty() {
            format!(
                "SITE: document.readyState reached \"complete\" after {:?} at {} but \
                 document.title was empty — the page loaded and answered with no title \
                 (rate limit, interstitial, or outage), so this is the site, not our readiness",
                self.waited, self.href
            )
        } else {
            format!(
                "SITE: document completed at {} with title {:?}, expected {expected:?} — the \
                 page answered, with different content",
                self.href, self.title
            )
        }
    }
}

/// Poll the live document until `document.readyState` is `"complete"` and
/// `document.title` is non-empty, or `timeout` elapses (iter-242 Part C).
///
/// `navigate` reporting success is not the same claim as "the document is
/// there": the iteration-175 sweep saw `live_eval_on_hn` get `""` back from
/// `document.title` seconds after a successful navigate, and green three
/// minutes later in isolation. A bounded readiness wait removes the ambiguity
/// where it can, and where it cannot, [`DocumentState::diagnosis`] says which
/// side the remaining failure is on.
///
/// Deliberately not a retry of the *assertion*: an accepted retry would hide a
/// real readiness regression behind a second attempt. This waits for the
/// document, then the caller asserts once.
pub fn await_document_ready(port: u16, timeout: Duration) -> DocumentState {
    let started = std::time::Instant::now();
    let read = |script: &str| -> String {
        Command::new(ff_rdp_bin())
            .args(base_args(port))
            .args(["eval", script])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| serde_json::from_slice::<serde_json::Value>(&o.stdout).ok())
            .and_then(|j| j["results"].as_str().map(str::to_owned))
            .unwrap_or_default()
    };
    loop {
        let ready_state = read("document.readyState");
        let title = read("document.title");
        if ready_state == "complete" && !title.is_empty() {
            return DocumentState {
                ready_state,
                title,
                href: read("window.location.href"),
                waited: started.elapsed(),
            };
        }
        if started.elapsed() >= timeout {
            return DocumentState {
                ready_state,
                title,
                href: read("window.location.href"),
                waited: started.elapsed(),
            };
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

/// Owner-PID marker written inside every ff-rdp-managed profile dir; mirrors
/// the product's private `util::profile_dir::OWNER_PID_MARKER`.
///
/// Duplicated rather than imported because this crate ships no `[lib]` target
/// for an integration-test binary to import from — the same unavoidable
/// duplication [`SPAWNING_TEST_ENV`] carries. What was *avoidable*, and is
/// fixed here (iter-242 Theme E), is the same literal appearing in three
/// separate modules of this one test binary: `live_151_residual_leak`,
/// `live_168_drop_waits_for_exit` and the since-deleted `live_96` scan all
/// spelled it out independently.
pub const OWNER_PID_MARKER: &str = ".ff-rdp-owner-pid";

/// Owner-test marker (iter-151 Theme A), same duplication rationale as
/// [`OWNER_PID_MARKER`].
pub const OWNER_TEST_MARKER: &str = ".ff-rdp-owner-test";

/// Scan `root` for `ff-rdp-profile-*` directories whose owner-PID marker names
/// a still-alive process, as `(dir, pid, spawning_test)` triples.
///
/// The single copy of a scan that was previously duplicated across live
/// modules (iter-242 Theme E). `spawning_test` is `None` when the profile
/// carries no `.ff-rdp-owner-test` marker — which for a profile spawned under
/// the live suite is itself the finding, since every launch site is supposed
/// to route through [`ff_rdp_launch_command`].
pub fn live_owned_profile_dirs(root: &str) -> Vec<(PathBuf, u32, Option<String>)> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_str()
                .is_some_and(|n| n.starts_with("ff-rdp-profile-"))
        })
        .filter_map(|e| {
            let pid: u32 = std::fs::read_to_string(e.path().join(OWNER_PID_MARKER))
                .ok()?
                .trim()
                .parse()
                .ok()?;
            if !pid_alive(pid) {
                return None;
            }
            let test_name = std::fs::read_to_string(e.path().join(OWNER_TEST_MARKER))
                .ok()
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty());
            Some((e.path(), pid, test_name))
        })
        .collect()
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

/// Env var overriding [`kill_wait_timeout`] (iter-168), in **milliseconds**.
pub const KILL_WAIT_TIMEOUT_ENV: &str = "FF_RDP_TEST_KILL_WAIT_TIMEOUT_MS";

/// Default bound for [`kill_pid_and_wait`], in milliseconds (iter-168).
///
/// Sized from iteration-168 Theme A's measurements: on this project's macOS
/// dev machine the post-`SIGKILL` window in which `kill(pid, 0)` still reports
/// a headless Firefox as alive measured 16–27 ms across ten launches, at load
/// averages from 6.5 to 54. 5 s is ~185× the observed worst case — deliberately
/// generous, because the cost of over-waiting is bounded (the poll returns the
/// instant the pid goes away) while the cost of under-waiting is the flaky
/// precondition failure this iteration exists to remove.
pub const DEFAULT_KILL_WAIT_MS: u64 = 5_000;

/// How long [`kill_pid_and_wait`] waits for a signalled process to actually
/// disappear before giving up loudly. [`DEFAULT_KILL_WAIT_MS`] by default;
/// override with [`KILL_WAIT_TIMEOUT_ENV`].
pub fn kill_wait_timeout() -> Duration {
    kill_wait_timeout_from(std::env::var(KILL_WAIT_TIMEOUT_ENV).ok().as_deref())
}

/// Pure parse behind [`kill_wait_timeout`], split out so the override contract
/// is testable without mutating process-global env state — `cargo test` runs a
/// binary's tests on parallel threads, and `set_var`/`remove_var` from one of
/// them is visible to all the others.
///
/// Unparseable or zero values fall back to [`DEFAULT_KILL_WAIT_MS`] rather than
/// to "don't wait": a typo'd override must not silently restore the pre-168
/// behaviour this iteration removes.
pub fn kill_wait_timeout_from(raw: Option<&str>) -> Duration {
    Duration::from_millis(
        raw.and_then(|v| v.trim().parse::<u64>().ok())
            .filter(|ms| *ms > 0)
            .unwrap_or(DEFAULT_KILL_WAIT_MS),
    )
}

/// Poll cadence for [`wait_for_pid_exit_with`].
///
/// 1 ms: the window being waited out is ~20 ms, and this runs on *every* live test's teardown, so a
/// coarse cadence would add up to 100 ms × ~150 drops of pure sleeping to a
/// suite that already takes half an hour.
const KILL_WAIT_POLL_INTERVAL: Duration = Duration::from_millis(1);

/// Poll `alive` until it reports `false` or `timeout` elapses (iter-168).
///
/// Returns `Some(elapsed)` with how long the process took to disappear, or
/// `None` if it was still alive at the deadline. Always probes at least once,
/// so a zero timeout degrades to a single check rather than to "never checked"
/// — an already-dead pid must return immediately, not after a sleep.
///
/// Takes the liveness probe as a closure so the timeout path is testable
/// without an unkillable process (and without a real Firefox anywhere).
pub fn wait_for_pid_exit_with(
    timeout: Duration,
    mut alive: impl FnMut() -> bool,
) -> Option<Duration> {
    let started = std::time::Instant::now();
    loop {
        if !alive() {
            return Some(started.elapsed());
        }
        if started.elapsed() >= timeout {
            return None;
        }
        std::thread::sleep(KILL_WAIT_POLL_INTERVAL);
    }
}

/// Wait for `pid` to actually leave the process table, bounded by `timeout`.
///
/// Thin [`pid_alive`] binding of [`wait_for_pid_exit_with`]; see
/// [`kill_pid_and_wait`] for why anything waits at all.
pub fn wait_for_pid_exit(pid: u32, timeout: Duration) -> Option<Duration> {
    wait_for_pid_exit_with(timeout, || pid_alive(pid))
}

/// `SIGKILL` `pid`, then wait (bounded) for it to actually go away (iter-168).
///
/// [`kill_pid`] only *signals*: it returns in ~20 µs while the kernel takes
/// ~20 ms to finish tearing the process down, and the test process is not
/// Firefox's parent so it never reaps it either. During that window
/// `kill(pid, 0)` — the liveness probe behind [`pid_alive`], and behind
/// `live_96_profile_cleanup`'s owner-PID precondition — still reports the
/// process as alive, so `profiles prune --all` correctly refuses to delete a
/// profile it believes is in use and an unrelated test fails.
///
/// That is the whole of iteration-168: signal-and-hope where a bounded poll
/// belongs.
///
/// Never panics: this runs from `Drop`, including while an assertion is
/// unwinding, and a panic during unwind aborts the process — turning one
/// failing test into a suiteless run. The timeout path therefore *reports*
/// (see [`report_kill_wait_timeout`]) rather than asserting, and returns
/// nothing: every caller is a `Drop` with no recovery available, so a status
/// value here would be pure unread API surface.
pub fn kill_pid_and_wait(pid: u32) {
    kill_pid(pid);
    let timeout = kill_wait_timeout();
    if wait_for_pid_exit(pid, timeout).is_none() {
        report_kill_wait_timeout(pid, timeout);
    }
}

/// Loud diagnostic for [`kill_pid_and_wait`]'s give-up path (iter-168).
///
/// A silent give-up would recreate the pre-168 behaviour exactly — the process
/// stays alive, the next profile-scanning test fails, and nothing says why. So
/// this names the pid, the bound, the env var that raises it, and the test that
/// owned the process, which is what iter-151's `OWNER_TEST_MARKER` bought and
/// what made this defect diagnosable in the first place.
///
/// Writes through [`std::io::stderr`] rather than `eprintln!` because the
/// latter panics if the write fails, and this is reachable from a `Drop` on an
/// unwinding thread.
fn report_kill_wait_timeout(pid: u32, timeout: Duration) {
    let _ = writeln!(
        std::io::stderr(),
        "LiveFirefox: pid {pid} was still alive {:?} after SIGKILL (owner test: {}). \
         Raise {KILL_WAIT_TIMEOUT_ENV} (milliseconds) if this is a slow machine rather \
         than a wedged process; until it exits, its ff-rdp profile dir still reads as \
         owned by a live process and `profiles prune --all` will refuse to remove it.",
        timeout,
        current_test_name(),
    );
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

/// Retain every launch command, including failures that have no success PID.
/// The start row is written before spawning; an unmatched start is incomplete
/// evidence, never a successful cleanup assertion. Raw output bytes are kept.
pub(crate) fn recorded_launch_output(
    command: &mut Command,
    path: &Path,
    attempt: u8,
    port: u16,
) -> Result<Output, String> {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| format!("cannot open launch-attempt ledger {}: {e}", path.display()))?;
    let identity = serde_json::json!({
        "test_process": std::process::id(), "test": current_test_name(),
        "attempt": attempt, "port": port, "started": chrono_now_rfc3339(),
    });
    let append = |file: &mut std::fs::File, row: serde_json::Value| {
        let mut bytes = serde_json::to_vec(&row).map_err(std::io::Error::other)?;
        bytes.push(b'\n');
        file.write_all(&bytes)
    };
    append(
        &mut file,
        serde_json::json!({
            "phase": "start", "identity": identity,
            "program": command.get_program().to_string_lossy(),
            "args": command.get_args().map(|s| s.to_string_lossy()).collect::<Vec<_>>(),
        "home": command.get_envs()
            .find(|(key, _)| *key == "FF_RDP_HOME")
            .map_or_else(|| std::env::var_os("FF_RDP_HOME"), |(_, value)| value.map(std::ffi::OsStr::to_os_string)),
        }),
    )
    .map_err(|e| format!("cannot record launch start {}: {e}", path.display()))?;
    let output = command.output();
    let row = match &output {
        Ok(output) => serde_json::json!({
            "phase": "output", "identity": identity, "ended": chrono_now_rfc3339(),
            "status": output.status.to_string(), "success": output.status.success(),
            "stdout": output.stdout, "stderr": output.stderr,
        }),
        Err(error) => serde_json::json!({
            "phase": "error", "identity": identity, "ended": chrono_now_rfc3339(),
            "error": error.to_string(),
        }),
    };
    if let Err(error) = append(&mut file, row) {
        // Preserve the captured bytes in the failing diagnostic and clean any
        // valid success receipt. A failed ledger never causes another launch.
        let _guard = output.as_ref().ok().and_then(guard_launched_firefox);
        return Err(format!(
            "cannot record launch outcome {}: {error}; captured output: {output:?}",
            path.display()
        ));
    }
    output.map_err(|e| format!("launch command failed: {e}"))
}

/// A failed-launch probe must retain its actual home even while unwinding.
/// With a capture ledger, put it beside that ledger so the occurrence census
/// can discover every profile. This path deliberately has no deleting Drop.
pub(crate) fn retained_failed_launch_home(ledger: Option<&Path>) -> std::io::Result<PathBuf> {
    let mut builder = tempfile::Builder::new();
    builder.prefix("ff-rdp-failed-launch-");
    let home = match ledger {
        Some(path) => builder.tempdir_in(
            path.parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or(Path::new(".")),
        )?,
        None => builder.tempdir()?,
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(home.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(home.keep())
}

/// In a fresh exclusive failed-launch home every surviving managed profile is
/// unexpected, even if its marker still names a live process. Never delete the
/// evidence here; a failed assertion must leave it for the owning supervisor.
pub(crate) fn assert_no_managed_profiles(root: &Path) {
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => panic!(
            "cannot inspect failed-launch profiles {}: {error}",
            root.display()
        ),
    };
    let mut survivors: Vec<_> = entries
        .map(|entry| {
            entry
                .unwrap_or_else(|error| panic!("cannot inspect profile entry: {error}"))
                .path()
        })
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("ff-rdp-profile-"))
        })
        .collect();
    survivors.sort();
    assert!(
        survivors.is_empty(),
        "failed launch left managed profiles (evidence retained): {survivors:?}"
    );
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

/// One caller-requested Firefox profile preference, serialized as JavaScript
/// syntax before `ff-rdp launch` starts Firefox. The product itself remains
/// responsible for its baseline DevTools preferences.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfilePreference {
    Bool(bool),
    String(String),
    Number(i64),
}

impl ProfilePreference {
    fn user_js_value(&self) -> String {
        match self {
            Self::Bool(value) => value.to_string(),
            Self::String(value) => {
                serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_owned())
            }
            Self::Number(value) => value.to_string(),
        }
    }
}

/// Facts recorded from one isolated harness launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveLaunchReceipt {
    pub ff_rdp_binary: PathBuf,
    pub firefox_binary: PathBuf,
    pub firefox_version: String,
    pub profile: PathBuf,
    pub pid: u32,
    pub port: u16,
    pub requested_preferences: Vec<(String, ProfilePreference)>,
}

/// A private ff-rdp home, profile and browser for one test.
///
/// This is deliberately adjacent to [`LiveFirefox`] instead of a second
/// launcher: it uses the product's `launch`.  `finish` is the observable
/// cleanup point; `Drop` only supplies a non-panicking last resort for
/// assertion-unwind paths.
pub struct IsolatedLiveFirefox {
    firefox: Option<LiveFirefox>,
    ff_rdp_binary: PathBuf,
    home: Option<tempfile::TempDir>,
    finished: bool,
    receipt: LiveLaunchReceipt,
}

impl IsolatedLiveFirefox {
    /// Launch through this exact compiled `ff-rdp` binary with an isolated
    /// `FF_RDP_HOME` and a fresh profile whose required prefs exist before the
    /// product launches Firefox. Firefox itself has no supported CLI option
    /// for a binary path, so the receipt records the path the product resolved.
    pub fn launch(ff_rdp_binary: &Path) -> Result<Self, String> {
        Self::launch_with_preferences(ff_rdp_binary, &[])
    }

    /// Like [`launch`](Self::launch), with caller-owned preferences written
    /// into the private profile before Firefox starts.
    pub fn launch_with_preferences(
        ff_rdp_binary: &Path,
        preferences: &[(String, ProfilePreference)],
    ) -> Result<Self, String> {
        let product_timeout = product_launch_timeout();
        let cleanup_allowance =
            kill_wait_timeout().max(Duration::from_millis(DEFAULT_KILL_WAIT_MS));
        let command_timeout = isolated_launch_command_timeout(product_timeout, cleanup_allowance);
        Self::launch_with_preferences_and_timeouts(
            ff_rdp_binary,
            preferences,
            product_timeout,
            command_timeout,
        )
    }

    /// Injected timeout half of [`launch_with_preferences`](Self::launch_with_preferences).
    ///
    /// Kept crate-visible so the Firefox-free harness regression can exercise
    /// the actual caller timeout and cleanup path in milliseconds.
    pub(crate) fn launch_with_preferences_and_timeouts(
        ff_rdp_binary: &Path,
        preferences: &[(String, ProfilePreference)],
        product_timeout: Duration,
        command_timeout: Duration,
    ) -> Result<Self, String> {
        let ff_rdp_binary = validate_session_binary(ff_rdp_binary)?;
        let home = tempfile::tempdir().map_err(|e| format!("create isolated FF_RDP_HOME: {e}"))?;
        let profile = home.path().join("profile");
        write_requested_profile_prefs(&profile, preferences)?;
        let port = free_port().ok_or_else(|| "reserve a random debugger port".to_owned())?;
        let product_timeout_secs = product_timeout.as_secs().to_string();

        let launch_result = bounded_command_output(
            Command::new(&ff_rdp_binary)
                .env("FF_RDP_HOME", home.path())
                .env(SPAWNING_TEST_ENV, current_test_name())
                .args([
                    "launch",
                    "--headless",
                    "--debug-port",
                    &port.to_string(),
                    "--profile",
                    &profile.to_string_lossy(),
                    "--launch-timeout",
                    &product_timeout_secs,
                ]),
            command_timeout,
            "isolated launch",
        )
        .and_then(|output| {
            if !output.status.success() {
                return Err(format!(
                    "isolated launch exited {} (binary {}): stdout={} stderr={}",
                    output.status,
                    ff_rdp_binary.display(),
                    String::from_utf8_lossy(&output.stdout).trim(),
                    String::from_utf8_lossy(&output.stderr).trim()
                ));
            }
            launch_receipt_from_output(&output.stdout, port, &profile)
                .map_err(|reason| format!("isolated launch receipt: {reason}"))
        });
        let mut receipt = match launch_result {
            Ok(receipt) => receipt,
            Err(reason) => {
                return Err(failed_launch_error(&reason, home, port));
            }
        };
        receipt.ff_rdp_binary.clone_from(&ff_rdp_binary);
        receipt.requested_preferences = preferences.to_vec();
        // A validated receipt is the single ownership transition. Every
        // subsequent failure cleans through the fully-owned session.
        let mut session = Self {
            firefox: Some(LiveFirefox {
                firefox_pid: receipt.pid,
                port,
            }),
            ff_rdp_binary,
            home: Some(home),
            finished: false,
            receipt,
        };
        if !wait_for_tcp(port, launch_wait_timeout()) {
            let pid = session.receipt.pid;
            let cleanup = session.cleanup().err().unwrap_or_default();
            session.finished = true;
            return Err(format!(
                "isolated launch reported pid {} but port {port} did not open within {:?}; cleanup: {cleanup}",
                pid,
                launch_wait_timeout()
            ));
        }
        session.receipt.firefox_version =
            match firefox_version_within(&session.receipt.firefox_binary, Duration::from_secs(5)) {
                Ok(version) => version,
                Err(reason) => {
                    let cleanup = session.cleanup().err().unwrap_or_default();
                    session.finished = true;
                    return Err(format!("{reason}; cleanup: {cleanup}"));
                }
            };
        Ok(session)
    }

    pub fn firefox(&self) -> &LiveFirefox {
        self.firefox
            .as_ref()
            .expect("isolated session used after finish")
    }

    pub fn receipt(&self) -> &LiveLaunchReceipt {
        &self.receipt
    }

    /// A command using this session's exact binary and private state root.
    pub fn command(&self) -> Command {
        let mut command = Command::new(&self.ff_rdp_binary);
        if let Some(home) = self.home.as_ref() {
            command.env("FF_RDP_HOME", home.path());
        }
        command
    }

    /// Stop only this session's browser and remove only its temporary
    /// profile root. Cleanup errors are returned to the caller for assertion.
    pub fn finish(mut self) -> Result<(), String> {
        let result = self.cleanup();
        self.finished = true;
        result
    }

    fn cleanup(&mut self) -> Result<(), String> {
        let mut failures = Vec::new();
        if let Some(ff) = self.firefox.take() {
            // The receipt's PID is the Firefox this session launched; there is
            // ff-rdp keeps no other handle on it.
            kill_pid_and_wait(ff.firefox_pid);
            if pid_alive(ff.firefox_pid) {
                failures.push(format!(
                    "Firefox pid {} remains alive after kill; preserving isolated root for inspection",
                    ff.firefox_pid
                ));
            }
            // Prevent LiveFirefox's Drop from signalling a recycled PID.
            std::mem::forget(ff);
        }
        if let Some(home) = self.home.take() {
            if failures.is_empty() {
                if let Err(e) = home.close() {
                    failures.push(format!("remove isolated FF_RDP_HOME: {e}"));
                }
            } else {
                let preserved = home.path().to_path_buf();
                std::mem::forget(home);
                failures.push(format!(
                    "preserved isolated FF_RDP_HOME at {}",
                    preserved.display()
                ));
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures.join("; "))
        }
    }
}

impl Drop for IsolatedLiveFirefox {
    fn drop(&mut self) {
        if !self.finished
            && let Err(reason) = self.cleanup()
        {
            let _ = writeln!(
                std::io::stderr(),
                "IsolatedLiveFirefox cleanup failed: {reason}"
            );
        }
    }
}

pub(crate) fn validate_session_binary(binary: &Path) -> Result<PathBuf, String> {
    if binary.as_os_str().is_empty() {
        return Err("isolated session requires a non-empty exact ff-rdp binary path".to_owned());
    }
    let absolute = if binary.is_absolute() {
        binary.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| format!("resolve current directory for ff-rdp binary: {e}"))?
            .join(binary)
    };
    if !absolute.is_file() {
        return Err(format!(
            "isolated session ff-rdp binary is not a file: {}",
            absolute.display()
        ));
    }
    absolute.canonicalize().map_err(|e| {
        format!(
            "canonicalize isolated session ff-rdp binary {}: {e}",
            absolute.display()
        )
    })
}

/// Run a child within a fixed budget without allowing a full stdout/stderr pipe
/// to block its exit. Output goes to temporary files and is collected only
/// after the child has exited (or has been killed and reaped).
pub(crate) fn bounded_command_output(
    command: &mut Command,
    timeout: Duration,
    operation: &str,
) -> Result<Output, String> {
    bounded_command_output_with_poll(command, timeout, operation, Child::try_wait)
}

/// Injected polling half of [`bounded_command_output`].
///
/// The injection exists only to force the otherwise rare `try_wait` error in
/// a deterministic Firefox-free test. Every branch after `spawn` either sees
/// an already-reaped exit status or calls [`terminate_and_reap`].
pub(crate) fn bounded_command_output_with_poll(
    command: &mut Command,
    timeout: Duration,
    operation: &str,
    mut poll: impl FnMut(&mut Child) -> std::io::Result<Option<ExitStatus>>,
) -> Result<Output, String> {
    let mut stdout =
        tempfile::tempfile().map_err(|e| format!("create {operation} stdout capture: {e}"))?;
    let mut stderr =
        tempfile::tempfile().map_err(|e| format!("create {operation} stderr capture: {e}"))?;
    command
        .stdout(Stdio::from(stdout.try_clone().map_err(|e| {
            format!("clone {operation} stdout capture: {e}")
        })?))
        .stderr(Stdio::from(stderr.try_clone().map_err(|e| {
            format!("clone {operation} stderr capture: {e}")
        })?));
    // Resolve an unrepresentably large caller duration before spawning. Once
    // the child exists, every fallible polling path must terminate and reap it.
    let deadline = std::time::Instant::now()
        .checked_add(timeout)
        .ok_or_else(|| format!("{operation} timeout is too large: {timeout:?}"))?;
    let mut child = command
        .spawn()
        .map_err(|e| format!("spawn {operation}: {e}"))?;
    let status = loop {
        match poll(&mut child) {
            Ok(Some(status)) => break status,
            Ok(None) if std::time::Instant::now() >= deadline => {
                return Err(format!(
                    "{operation} timed out after {timeout:?}; {}",
                    terminate_and_reap(&mut child)
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(error) => {
                return Err(format!(
                    "poll {operation}: {error}; {}",
                    terminate_and_reap(&mut child)
                ));
            }
        }
    };
    stdout
        .rewind()
        .map_err(|e| format!("rewind {operation} stdout: {e}"))?;
    stderr
        .rewind()
        .map_err(|e| format!("rewind {operation} stderr: {e}"))?;
    let mut stdout_bytes = Vec::new();
    let mut stderr_bytes = Vec::new();
    stdout
        .read_to_end(&mut stdout_bytes)
        .map_err(|e| format!("read {operation} stdout: {e}"))?;
    stderr
        .read_to_end(&mut stderr_bytes)
        .map_err(|e| format!("read {operation} stderr: {e}"))?;
    Ok(Output {
        status,
        stdout: stdout_bytes,
        stderr: stderr_bytes,
    })
}

fn terminate_and_reap(child: &mut Child) -> String {
    let child_pid = child.id();
    let kill_error = child.kill().err();
    let reap = child.wait();
    format!("child_pid={child_pid}; kill={kill_error:?}; reap={reap:?}")
}

/// Diagnostic display only: null means no exact UTF-8 representation exists.
/// Cleanup must continue to use the original Path, never this display value.
pub(crate) fn launch_request_context(home: &Path, port: u16) -> serde_json::Value {
    serde_json::json!({
        "home": home.to_str(),
        "home_display": home.to_string_lossy(),
        "home_display_is_lossy": home.to_str().is_none(),
        "port": port,
    })
}

/// Finish every failure after the launch command was invoked. No receipt
/// field, especially its PID, is trusted; the private home and requested port
/// are the authority. A failed `launch` reaps the Firefox it spawned itself
/// (iter-282), so a port that is still listening means something survived:
/// the home is preserved for inspection rather than removed under it.
pub(crate) fn failed_launch_error(reason: &str, home: tempfile::TempDir, port: u16) -> String {
    // Parent-selected authority survives even when the launch child never
    // executes or writes a receipt. Keep it separate from cleanup's response.
    let request = launch_request_context(home.path(), port);
    let reason = format!("isolated launch request: {request}\n{reason}");
    if wait_for_tcp_closed(port, Duration::from_secs(2)) {
        let removal = home.close().map_or_else(
            |e| format!("could not remove isolated FF_RDP_HOME: {e}"),
            |()| "removed isolated FF_RDP_HOME".to_owned(),
        );
        format!("{reason}; port {port} is free; {removal}")
    } else {
        let preserved = home.path().to_path_buf();
        std::mem::forget(home);
        format!(
            "{reason}; port {port} is still listening after the failed launch; preserving {} for inspection",
            preserved.display()
        )
    }
}

pub(crate) fn write_requested_profile_prefs(
    profile: &Path,
    preferences: &[(String, ProfilePreference)],
) -> Result<(), String> {
    std::fs::create_dir_all(profile)
        .map_err(|e| format!("create isolated profile {}: {e}", profile.display()))?;
    let mut user_js = std::fs::File::create(profile.join("user.js"))
        .map_err(|e| format!("create isolated profile preferences: {e}"))?;
    for (name, value) in preferences {
        writeln!(user_js, "user_pref({name:?}, {});", value.user_js_value())
            .map_err(|e| format!("write required Firefox preference {name}: {e}"))?;
    }
    Ok(())
}

pub(crate) fn launch_receipt_from_output(
    stdout: &[u8],
    requested_port: u16,
    requested_profile: &Path,
) -> Result<LiveLaunchReceipt, String> {
    if requested_port == 0 || requested_profile.as_os_str().is_empty() {
        return Err(
            "isolated launch requires a non-zero requested port and non-empty profile".to_owned(),
        );
    }
    let json: serde_json::Value = serde_json::from_slice(stdout)
        .map_err(|e| format!("isolated launch did not return JSON: {e}"))?;
    let get_string = |path: &str| {
        json.pointer(path)
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| format!("isolated launch receipt lacks {path}"))
    };
    let pid = json
        .pointer("/results/pid")
        .and_then(serde_json::Value::as_u64)
        .and_then(|p| u32::try_from(p).ok())
        .filter(|pid| *pid != 0)
        .ok_or_else(|| "isolated launch receipt lacks non-zero numeric /results/pid".to_owned())?;
    let port = json
        .pointer("/results/port")
        .and_then(serde_json::Value::as_u64)
        .and_then(|p| u16::try_from(p).ok())
        .filter(|port| *port != 0)
        .ok_or_else(|| "isolated launch receipt lacks non-zero valid /results/port".to_owned())?;
    let firefox_binary = PathBuf::from(get_string("/meta/firefox")?);
    let profile = PathBuf::from(get_string("/results/profile")?);
    if firefox_binary.as_os_str().is_empty() {
        return Err("isolated launch receipt has an empty /meta/firefox".to_owned());
    }
    if port != requested_port || profile != requested_profile {
        return Err(format!(
            "receipt does not match requested port/profile (got port {port}, profile {}; expected port {requested_port}, profile {})",
            profile.display(),
            requested_profile.display()
        ));
    }
    Ok(LiveLaunchReceipt {
        ff_rdp_binary: PathBuf::new(),
        firefox_binary,
        firefox_version: String::new(),
        profile,
        pid,
        port,
        requested_preferences: Vec::new(),
    })
}

fn firefox_version_within(binary: &Path, timeout: Duration) -> Result<String, String> {
    let output = bounded_command_output(
        Command::new(binary).arg("--version"),
        timeout,
        "Firefox version command",
    )?;
    if !output.status.success() {
        return Err(format!(
            "Firefox version command exited {} for {}: {}",
            output.status,
            binary.display(),
            output_note(&output)
        ));
    }
    let version = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (!version.is_empty()).then_some(version).ok_or_else(|| {
        format!(
            "Firefox version command returned empty output for {}",
            binary.display()
        )
    })
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

    /// One attempt shared by the panicking and fallible entry points. A failed
    /// launch is evidence to inspect, not permission to hide it behind a later
    /// success (which may coexist with an unaccounted first browser).
    fn try_headless_on_random_port_with_args(
        extra_args: &[&str],
    ) -> Result<(Self, serde_json::Value), Vec<String>> {
        Self::try_launch(extra_args, 0).map_err(|diagnostic| vec![diagnostic])
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

        let ledger = std::env::var_os(LIVE_LAUNCH_LOG_ENV)
            .map_or_else(
                || ff_rdp_bin().parent().unwrap().join("../live-launches.log"),
                PathBuf::from,
            )
            .with_extension("attempts.jsonl");
        let mut command = Command::new(ff_rdp_bin());
        command
            .args(["launch", "--headless", "--debug-port", &port.to_string()])
            .args(extra_args)
            // iter-151 Theme A: identify the spawning test so a leaked
            // profile is traceable from the artifact alone — see
            // `SPAWNING_TEST_ENV`'s doc comment.
            .env(SPAWNING_TEST_ENV, current_test_name());
        let output = recorded_launch_output(&mut command, &ledger, attempt, port).map_err(|e| {
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
            // iter-168: this abandoned launch already planted an owner-PID
            // marker, so it must be *gone* — not merely signalled — before the
            // retry (or a later profile-scanning test) looks at the root.
            kill_pid_and_wait(firefox_pid);
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
}

pub fn output_note(out: &std::process::Output) -> String {
    format!(
        "status={:?} stdout={} stderr={}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).trim(),
        String::from_utf8_lossy(&out.stderr).trim()
    )
}

impl Drop for LiveFirefox {
    fn drop(&mut self) {
        // iter-168: wait for the process to actually go away, not just for the
        // signal to be delivered. Every live test inherits this drop, so the
        // race lived here rather than in any one test — see
        // `kill_pid_and_wait`.
        kill_pid_and_wait(self.firefox_pid);
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
        //
        // iter-168: this path needs no `kill_pid_and_wait`. `Child::wait`
        // blocks until the process has actually terminated, which is strictly
        // stronger than polling `kill(pid, 0)` — `RawFirefox` is a direct child
        // of the test process, whereas `LiveFirefox`'s Firefox is not, which is
        // exactly why the latter had to poll.
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

/// Return the host's current load-average text for timing-test diagnostics.
///
/// This runs only after the timed operation, so collecting the diagnostic
/// cannot inflate the measurement it explains. Platforms without `uptime`
/// retain an explicit unavailable value rather than hiding the sample.
pub fn timing_load_note() -> String {
    std::process::Command::new("uptime")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .filter(|output| !output.is_empty())
        .unwrap_or_else(|| "load average unavailable".to_owned())
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
            for chunk in buf.as_chunks::<3>().0 {
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
    /// How long to sit on the request before writing the first byte (iter-220).
    ///
    /// Zero by default, which is every pre-iter-220 fixture. A non-zero delay
    /// buys a navigation that is genuinely *in flight* for a measurable window:
    /// the existing two-element fixtures commit before a collector can race
    /// them, which is exactly why three live suites and two iterations missed
    /// `--with-page` collecting from the outgoing document. Set it with
    /// [`FixtureRoute::with_delay`].
    pub delay: Duration,
}

impl FixtureRoute {
    /// Convenience constructor for `text/html; charset=utf-8` routes, which
    /// is what every current fixture-server consumer serves.
    pub fn html(body: impl Into<String>) -> Self {
        Self {
            content_type: "text/html; charset=utf-8",
            body: body.into().into_bytes(),
            extra_headers: Vec::new(),
            delay: Duration::ZERO,
        }
    }

    /// Builder method: hold the response back for `delay` before the first
    /// byte, so the navigation to this route stays uncommitted that long.
    pub fn with_delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
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

        // iter-220: one thread per connection. With a delayed route the accept
        // loop would otherwise be the delay's hostage — Firefox's parallel
        // subresource and favicon requests would queue behind it and the page
        // would appear far slower than the route asked for.
        let routes = Arc::new(routes);
        let handle = std::thread::spawn(move || {
            let mut workers = Vec::new();
            for stream in listener.incoming() {
                if shutdown_for_thread.load(Ordering::Acquire) {
                    break;
                }
                let Ok(stream) = stream else { continue };
                let routes = Arc::clone(&routes);
                workers.push(std::thread::spawn(move || {
                    handle_connection(stream, &routes);
                }));
            }
            for worker in workers {
                let _ = worker.join();
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
        if !route.delay.is_zero() {
            std::thread::sleep(route.delay);
        }
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
