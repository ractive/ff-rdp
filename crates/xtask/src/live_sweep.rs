//! `xtask live-sweep`
//!
//! Filed as iter-155: a skipped live test reports green, so a green live
//! sweep can mean "did not run".
//!
//! ## The defect
//!
//! Live tests gate themselves twice: `#[ignore]` (skipped unless
//! `--include-ignored`), and an early runtime `return` when
//! `FF_RDP_LIVE_TESTS` / `FF_RDP_LIVE_NETWORK_TESTS` is unset. The second gate
//! is the problem: libtest counts a test that returns without panicking as
//! **passed**, not ignored. `FF_RDP_LIVE_TESTS=1 cargo test-live` (which
//! blanket-passes `--include-ignored`) therefore reports every
//! network-gated test as `ok` even when `FF_RDP_LIVE_NETWORK_TESTS` is unset
//! — the summary line `N passed; 0 failed` cannot be told apart from `N`
//! tests having actually exercised Firefox.
//!
//! ## The fix
//!
//! This module never touches the ~90 individual `#[ignore]`-gated test
//! bodies (their internal `if !live_tests_enabled() { return; }` checks stay
//! exactly as they are — now merely redundant, not load-bearing). Instead it
//! statically classifies each gated test from its own `#[ignore = "…"]`
//! reason text (an iter-155 audit found every current reason under
//! `tests/live/` names at least one of `FF_RDP_LIVE_TESTS` /
//! `FF_RDP_LIVE_NETWORK_TESTS` — a reliable, low-maintenance signal, cheaper
//! than re-deriving it from the varying body idioms), then drives `cargo
//! test` in two phases per target:
//!
//! 1. **Qualified** tests (every env var they need is actually set) run for
//!    real, with `--include-ignored` — libtest reports genuine `ok`/`FAILED`.
//! 2. **Unqualified** tests are selected by exact name *without*
//!    `--include-ignored` — since they still carry `#[ignore]` and nothing
//!    forces them to run, libtest reports them as `ignored`, using stable
//!    Rust's own vocabulary, not a fabricated status.
//!
//! The executed count (Theme B) is therefore not inferred from prose — it is
//! `qualified.len()`, known before a single `cargo test` process is spawned,
//! and is `0` whenever the relevant env vars are unset.
//!
//! ## Concurrency (iter-188 Theme C)
//!
//! Phase 1 used to pass `--test-threads=1` unconditionally, so the sweep ran
//! ~280 live tests one at a time and took 38 minutes. Iteration 188 measured
//! where that time goes: a headless Firefox cold start costs 5.64 s ± 0.02,
//! the tier performs ~200 of them, and only 8% of tests finish in under 6 s —
//! i.e. roughly half the wall clock is browsers starting up, and the machine
//! is idle for most of it.
//!
//! Phase 1 now runs [`Args::jobs`] tests concurrently, with two deliberate
//! exceptions:
//!
//! - **Targets whose tests need the port-6000 Firefox stay serial.** They all
//!   drive one browser somebody else started; running them concurrently would
//!   interleave several RDP clients on a single connection, and iter-173's
//!   "did the browser vanish mid-tier?" inference assumes one test at a time.
//!   See [`jobs_for_target`].
//! - **Phase 2 is unaffected**: it deliberately omits `--include-ignored` so
//!   libtest reports those tests `ignored` without running them, and a
//!   thread count for tests that never execute would be noise.
//!
//! This drives libtest's own `--test-threads` rather than shelling out to
//! `cargo nextest`. nextest would additionally give process-per-test
//! isolation, per-test timings and per-test timeouts — all real — but it is a
//! second test runner with a different failure-output format, and every one of
//! this module's accounting guarantees ([`classify_failures`],
//! [`failure_blocks`], the `executed`/`skipped`/`preexisting`/`vanished`/
//! `launch_timeout` tiers) is written against libtest's. Re-deriving them
//! against a second format is exactly the kind of change that makes a gate lie
//! about what passed, which is the failure class this module exists to
//! prevent. libtest's in-process threads are also *not* free of hazards: see
//! `kb/iterations/iteration-196-frame-cap-lock-has-no-readers.md`. The live
//! tier is safe for them because it spawns Firefox as a child process per test
//! and the harness deliberately never mutates process-global env
//! (`tests/common/mod.rs`, `kill_wait_timeout_from` / `parse_launch_timeout`).
//!
//! ## The watchdog (iter-197)
//!
//! Iteration 188's third sweep hung: libtest printed `test
//! live_158_launch_survives_contended_bind has been running for over 60
//! seconds` after 276 of 277 CLI-tier tests and then waited **forever**, with
//! four Firefox processes still open and no `LIVE_SWEEP_SUMMARY` ever printed.
//! libtest has no per-test timeout of any kind, and neither had this module,
//! so nothing in the pipeline had a bound. For an unattended loop that is
//! strictly worse than a red: a red costs one iteration, a hang costs the
//! night.
//!
//! [`run_phase`] now bounds *silence* rather than wall clock (see
//! [`DEFAULT_PHASE_STALL_SECS`]), kills the phase's process group, names the
//! tests that never reported a verdict ([`unreported_tests`]), reaps the
//! browsers the kill cannot reach ([`reap_managed_firefox`]), and books them
//! into a [`SweepSummary::timed_out`] tier so `total` stays conserved and the
//! sweep still exits non-zero with a summary line.
//!
//! ### Why a watchdog and not `cargo nextest`
//!
//! nextest runs each test in its own process and ships `slow-timeout` +
//! `terminate-after`, which is a per-*test* bound where this is a per-*phase*
//! one. Iteration 188 declined it on cost grounds; iteration 197 re-opened the
//! question with the hang as evidence and still declined it, on the grounds
//! the plan asked for — the accounting, not the wall clock:
//!
//! - Every tier this module reports (`executed` / `skipped` / `preexisting` /
//!   `vanished` / `launch_timeout`) is derived from libtest's exact output:
//!   [`failure_blocks`] parses `---- <name> stdout ----` headers,
//!   [`classify_failures`] matches panic prose inside them, and phase 2's
//!   whole design rests on libtest printing `ignored` for an `#[ignore]` test
//!   selected without `--include-ignored`. nextest has a different failure
//!   format and a different notion of "ignored", so adopting it means
//!   re-deriving five tiers against a second parser — precisely the change
//!   most likely to make the gate lie about what passed, which is the failure
//!   class this module exists to prevent (iter-155).
//! - The watchdog needs none of that. It reads *one* new thing — which of the
//!   names the sweep itself passed to `--exact` never got a verdict line — and
//!   that is a set difference against the sweep's own input, not prose.
//! - nextest would also be a required dev tool on every machine that closes an
//!   iteration, and `cargo test` is what CLAUDE.md's gates already run.
//!
//! What is genuinely given up: a per-test bound would kill only the hung test
//! and let its ~275 siblings finish, where the watchdog kills the phase and
//! books the rest as `timed_out`. That is the honest cost, and it is the right
//! trade while a hang is a once-in-three-sweeps event — it converts an
//! unbounded hang into a bounded red without touching a single accounting
//! guarantee. If hangs become routine, this is the paragraph to revisit.
//!
//! `FF_RDP_LIVE_TESTS_RECORD`-driven fixture recording
//! (`ff-rdp-core/tests/live_record_fixtures.rs`) is out of scope: it has its
//! own documented one-off workflow and a third env var this classifier does
//! not model.

use anyhow::{Context, Result, anyhow};
use clap::Args as ClapArgs;
use std::path::{Path, PathBuf};
use std::process::Command;

// ---------------------------------------------------------------------------
// Args
// ---------------------------------------------------------------------------

#[derive(ClapArgs)]
pub struct Args {
    /// Workspace root. xtask subcommands are invoked via `cargo run -p xtask`
    /// from the repo root, matching every other `check-*` subcommand's
    /// default-relative-path convention.
    #[arg(long, default_value = ".")]
    pub workspace_root: PathBuf,

    /// Print the qualified/unqualified plan and the resulting executed count
    /// without invoking `cargo test`.
    #[arg(long)]
    pub dry_run: bool,

    /// How many live tests phase 1 runs concurrently (libtest
    /// `--test-threads`), for targets that launch their own Firefox.
    ///
    /// Defaults to the measured knee of 6, capped by the machine's own
    /// parallelism (see `default_jobs`). Pass `--jobs 1` to reproduce the
    /// pre-188 serial sweep.
    ///
    /// An explicit `--jobs` is **not** clamped to [`MAX_SWEEP_JOBS`] — that
    /// cap only shapes the *default*. Passing `--jobs 8` or higher is a
    /// deliberate escape hatch, not a recommendation: iteration 188 measured
    /// 8 workers manufacturing four contention-only failures on a 10-core
    /// machine that do not occur at 6 (see [`MAX_SWEEP_JOBS`]'s doc), so a
    /// higher value trades the gate's "does not lie about what passed"
    /// property for wall clock. Only reach for it with reason to believe the
    /// box underneath is meaningfully bigger than the one that set the cap.
    #[arg(long, default_value_t = default_jobs())]
    pub jobs: usize,

    /// Watchdog bound (seconds) on how long one phase's stdout may stay
    /// **silent after libtest has started reporting results** before the
    /// sweep declares the phase hung, kills it, and moves on (iter-197).
    ///
    /// `0` disables the watchdog entirely and restores the pre-197
    /// wait-forever behaviour — useful when attaching a debugger to a hung
    /// test, never appropriate for an unattended run.
    ///
    /// See [`DEFAULT_PHASE_STALL_SECS`] for why the default is what it is.
    #[arg(long, default_value_t = DEFAULT_PHASE_STALL_SECS)]
    pub phase_stall_secs: u64,

    /// Watchdog bound (seconds) on the silence *before* a phase's first
    /// libtest line — the window in which `cargo` is compiling and stdout is
    /// legitimately empty (cargo's progress goes to stderr, which this tool
    /// inherits and never reads).
    ///
    /// Deliberately much larger than `--phase-stall-secs`: a cold
    /// `cargo test --test live` build of this workspace is minutes of silence
    /// that must not be mistaken for a hang. `0` disables it.
    #[arg(long, default_value_t = DEFAULT_PHASE_BUILD_SECS)]
    pub phase_build_secs: u64,
}

/// Default stall bound (seconds): the longest silence between two libtest
/// result lines that is still treated as progress.
///
/// Justified against iteration 188's per-test timing census of this exact
/// corpus (n=277, nextest `-j4`): mean 8.83 s, median 7.68 s, p90 12.40 s,
/// **p99 38.20 s, max 43.43 s**. The stall clock is reset by *every* line
/// libtest prints, so in the worst case — a serial phase, one test in flight —
/// the gap between two lines is one test's wall time. 300 s is **7.9× the
/// measured p99 and 6.9× the measured max**, which is the margin the "a bound
/// that fires every run is worse than no bound" rule asks for: a test would
/// have to become seven times slower than the slowest one ever measured here
/// before the watchdog produced a false positive.
///
/// It is also small enough to matter. The hang this bound exists for
/// (iteration 188's third sweep) burned the remainder of a 60-minute harness
/// timeout; at 300 s the same sweep loses five minutes and still prints a
/// `LIVE_SWEEP_SUMMARY`.
pub const DEFAULT_PHASE_STALL_SECS: u64 = 300;

/// Default bound (seconds) on the silence before a phase's *first* stdout
/// line, i.e. the `cargo` build.
///
/// Separate from [`DEFAULT_PHASE_STALL_SECS`] because the two windows measure
/// different things: nothing is running yet, so the p99 test time says
/// nothing about how long this may legitimately take. 15 minutes covers a
/// cold-cache debug build of the whole workspace plus the live test target on
/// a slow machine, and still bounds the one genuine hang mode here (a `cargo`
/// blocked forever on the target-directory lock held by another build).
pub const DEFAULT_PHASE_BUILD_SECS: u64 = 900;

/// The two watchdog bounds a phase runs under, resolved from [`Args`].
///
/// `Duration::ZERO` on either field means "unbounded" — see the flag docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhaseBounds {
    /// Max silence before the first libtest line (the build window).
    pub build: std::time::Duration,
    /// Max silence between libtest lines once results have started arriving.
    pub stall: std::time::Duration,
}

impl PhaseBounds {
    /// Read the bounds off the parsed CLI arguments.
    pub fn from_args(args: &Args) -> Self {
        Self {
            build: std::time::Duration::from_secs(args.phase_build_secs),
            stall: std::time::Duration::from_secs(args.phase_stall_secs),
        }
    }

    /// The bound that applies right now: the build window until libtest has
    /// printed something, the stall window afterwards. `None` means the
    /// applicable bound is disabled and the wait is unbounded.
    fn current(&self, seen_output: bool) -> Option<std::time::Duration> {
        let d = if seen_output { self.stall } else { self.build };
        (!d.is_zero()).then_some(d)
    }
}

impl Default for PhaseBounds {
    fn default() -> Self {
        Self {
            build: std::time::Duration::from_secs(DEFAULT_PHASE_BUILD_SECS),
            stall: std::time::Duration::from_secs(DEFAULT_PHASE_STALL_SECS),
        }
    }
}

/// Concurrency ceiling for phase 1 (iter-188 Theme C).
///
/// Chosen from repeated whole-tier runs on a 10-core / 32 GB machine, not
/// from a single one: at 8 workers four extra tests fail
/// (`live_emulate_color_scheme_dark`, `live_137_consent_accept_via_daemon`,
/// `live_138_back_forward_committed_url_is_top_frame`,
/// `live_runner_page_map_resolution`) purely from contention. A gate whose
/// job is to not lie about what passed cannot be allowed to manufacture reds,
/// so the extra ~15% of wall clock is refused.
pub const MAX_SWEEP_JOBS: usize = 6;

/// Default phase-1 concurrency: [`MAX_SWEEP_JOBS`], but never more than the
/// machine reports it can run in parallel — a 2-core CI box oversubscribed 3×
/// with Firefox processes is how launch timeouts get manufactured.
pub fn default_jobs() -> usize {
    std::thread::available_parallelism()
        .map(std::num::NonZeroUsize::get)
        // `unwrap_or(1)` already floors this, so `min` alone is the whole
        // clamp — spelling it as `clamp(1, ..)` trips `clippy::manual_clamp`.
        .unwrap_or(1)
        .min(MAX_SWEEP_JOBS)
}

/// Phase-1 concurrency for one target.
///
/// A target whose tests connect to the Firefox on [`PREEXISTING_PORT`] runs
/// serially no matter what `--jobs` says: those tests share one browser they
/// did not start, so concurrency would have several of them issuing RDP
/// commands over one connection, and the vanished-browser inference
/// ([`repartition_for_probe`], [`classify_failures`]) is written for a tier
/// that runs one test at a time.
pub fn jobs_for_target(target_needs_preexisting: bool, requested: usize) -> usize {
    if target_needs_preexisting {
        1
    } else {
        requested.max(1)
    }
}

// ---------------------------------------------------------------------------
// Classification
// ---------------------------------------------------------------------------

/// A single `#[ignore]`-gated live test, classified by which env var(s) its
/// own ignore-reason text names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatedTest {
    /// Fully-qualified libtest name (`module::fn` for the consolidated
    /// `ff-rdp-cli` target, bare `fn` for `ff-rdp-core`'s per-file targets).
    pub full_name: String,
    pub needs_live: bool,
    pub needs_network: bool,
    /// The test connects to a Firefox somebody else started on the fixed
    /// default port; it never launches one itself (iter-158 Theme F).
    pub needs_preexisting: bool,
}

/// Default Firefox debug port. A test file that talks to this port without
/// launching anything needs an instance a human (or another tool) started.
pub const PREEXISTING_PORT: u16 = 6000;

/// Markers that identify a source file whose live tests connect to a
/// pre-existing Firefox on [`PREEXISTING_PORT`] rather than launching one.
///
/// Theme F's evidence: on 2026-08-13 six of the sweep's seven failures were
/// `ConnectionRefused` from `ff-rdp-core` live tests, all of which resolve
/// their port through `support::recording::firefox_port()` (default 6000) and
/// none of which spawns a browser — their own `#[ignore]` reasons and module
/// docs say so (`live_firefox_test.rs:26`, `live_61p_registry.rs:3`,
/// `live_129_frame_targets.rs:12`). `live-sweep` neither provided that
/// instance nor checked for it, and counted all of them as `executed`. That is
/// a **third** way `executed=N` overstated reality.
///
/// The markers are matched against the whole file, not just the `#[ignore]`
/// reason: the reasons are inconsistent across those files (only
/// `live_firefox_test.rs` spells out `--start-debugger-server 6000`), while
/// `firefox_port(` is a reliable signal in every one of them.
const PREEXISTING_MARKERS: &[&str] = &[
    "--start-debugger-server 6000",
    // `support::recording::firefox_port()` — defaults to 6000, overridable
    // via `FF_RDP_PORT`. Matched bare so both the `use` line and the call
    // site count.
    "firefox_port",
    "remote debugger enabled on port 6000",
];

/// Markers that prove a source file **launches its own Firefox**, which
/// overrides every [`PREEXISTING_MARKERS`] hit in the same file (iter-173
/// Task D).
///
/// The positive markers are bare substrings, and one of them — `firefox_port`
/// — is also the name of a field in `daemon.<port>.json`. Any `ff-rdp-cli`
/// live test that reads the registry back and asserts on that field was
/// therefore silently reclassified as needing a Firefox somebody else started
/// on port 6000, even though it launches one itself. iter-172 hit this while
/// writing `live_172_published_record_is_complete_and_lock_is_a_sibling`: the
/// two new tests moved into the `preexisting` bucket and tripped
/// `test_158_real_core_targets_are_preexisting`. The only workaround available
/// then was for the test to avoid writing the word, which the next author
/// would not know to do.
///
/// Consequence if left unfixed: a CLI test that merely mentions the field is
/// classified `preexisting`, so with nothing listening on 6000 it is reported
/// `ignored` instead of run — the same false-green shape as iter-155, reached
/// by a different road.
///
/// These two launcher types are the only ways a live test in this workspace
/// starts a browser (`common::LiveFirefox`, `common::RawFirefox`); no
/// `ff-rdp-core` live target mentions either, and 94 of the 97 `tests/live/`
/// files do.
const SELF_LAUNCH_MARKERS: &[&str] = &["LiveFirefox", "RawFirefox"];

/// Does this source file's live tests require a Firefox somebody else started?
///
/// A file that launches its own browser never does, whatever else it happens
/// to mention — see [`SELF_LAUNCH_MARKERS`].
pub fn source_needs_preexisting_instance(src: &str) -> bool {
    if SELF_LAUNCH_MARKERS.iter().any(|m| src.contains(m)) {
        return false;
    }
    PREEXISTING_MARKERS.iter().any(|m| src.contains(m))
}

/// Is something accepting TCP on `127.0.0.1:6000` right now?
///
/// Theme F decision: **classify, do not launch.** Port 6000 is ff-rdp's
/// documented default and the port a human is most likely to already be using
/// by hand; the fails-closed ownership guard in `daemon/client.rs` exists
/// precisely because ff-rdp once killed a hand-started Firefox on it. A sweep
/// that binds 6000 itself either collides with the user or inherits that whole
/// ownership problem. One TCP probe is honest about what it did.
pub fn preexisting_instance_available() -> bool {
    std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], PREEXISTING_PORT)),
        std::time::Duration::from_millis(500),
    )
    .is_ok()
}

/// Scan one file's source for `#[ignore = "REASON"]`-gated `#[test]`
/// functions whose reason mentions `FF_RDP_LIVE_TESTS` and/or
/// `FF_RDP_LIVE_NETWORK_TESTS`.
///
/// `module_prefix` is prepended as `"{prefix}::{fn}"` when the file is a
/// `mod` of a consolidated target (`ff-rdp-cli`'s `tests/live/*.rs`); pass
/// `None` for a file that is itself a standalone integration-test binary
/// (`ff-rdp-core`'s `tests/live_*.rs`).
///
/// The ignore attribute may span multiple lines (a reason string can use `\`
/// line continuation — see `live_daemon_watch_targets.rs`), and a `#[cfg(…)]`
/// may sit between the `#[ignore]` and the `fn`. Both are handled by scanning
/// the whole file as one string rather than line-by-line.
pub fn scan_source(src: &str, module_prefix: Option<&str>) -> Vec<GatedTest> {
    let mut out = Vec::new();
    let needs_preexisting = source_needs_preexisting_instance(src);
    let bytes = src.as_bytes();
    let mut pos = 0usize;

    while let Some(rel) = src[pos..].find("#[ignore") {
        let ignore_start = pos + rel;
        let mut cursor = ignore_start + "#[ignore".len();

        // Optional `= "REASON"` (REASON may contain escaped quotes/backslashes
        // and, via `\`-continuation, literal newlines).
        let mut reason = String::new();
        let after_ignore = skip_ws(src, cursor);
        if bytes.get(after_ignore) == Some(&b'=') {
            let after_eq = skip_ws(src, after_ignore + 1);
            if bytes.get(after_eq) == Some(&b'"') {
                let (parsed_reason, end) = parse_quoted_string(src, after_eq + 1);
                reason = parsed_reason;
                cursor = end;
            } else {
                cursor = after_eq;
            }
        }

        // Close the `#[ignore...]` attribute itself.
        let Some(close_rel) = src[cursor..].find(']') else {
            pos = ignore_start + 1;
            continue;
        };
        cursor += close_rel + 1;

        // Skip any intervening attributes (e.g. `#[cfg(unix)]`) and
        // whitespace, then require `fn <name>`.
        let mut scan = cursor;
        loop {
            scan = skip_ws(src, scan);
            if bytes.get(scan) == Some(&b'#') {
                if let Some(end_rel) = src[scan..].find(']') {
                    scan += end_rel + 1;
                    continue;
                }
                break;
            }
            break;
        }

        if reason.contains("FF_RDP_LIVE")
            && let Some(name) = parse_fn_name(src, scan)
        {
            let needs_network = reason.contains("FF_RDP_LIVE_NETWORK_TESTS");
            let mut needs_live = reason.contains("FF_RDP_LIVE_TESTS");
            if !needs_live && !needs_network {
                // Neither literal present despite matching "FF_RDP_LIVE" —
                // shouldn't happen per the iter-155 audit, but default to
                // the more conservative requirement rather than silently
                // treating the test as gate-free.
                needs_live = true;
            }
            let full_name = match module_prefix {
                Some(m) => format!("{m}::{name}"),
                None => name,
            };
            out.push(GatedTest {
                full_name,
                needs_live,
                needs_network,
                needs_preexisting,
            });
        }

        pos = ignore_start + 1;
    }

    out
}

fn skip_ws(src: &str, mut i: usize) -> usize {
    let bytes = src.as_bytes();
    while let Some(&b) = bytes.get(i) {
        if b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' {
            i += 1;
        } else {
            break;
        }
    }
    i
}

/// Parse a `"…"` string literal (with `\`-escapes, including `\`-newline
/// continuation) starting right after the opening quote. Returns the decoded
/// reason text and the index right after the closing quote.
fn parse_quoted_string(src: &str, start: usize) -> (String, usize) {
    let bytes = src.as_bytes();
    let mut out = String::new();
    let mut i = start;
    while let Some(&b) = bytes.get(i) {
        match b {
            b'"' => {
                return (out, i + 1);
            }
            b'\\' => {
                // Escaped char (or line-continuation newline) — keep the
                // literal text minus the backslash itself; good enough for
                // substring matching on env var names.
                if let Some(&next) = bytes.get(i + 1) {
                    if next != b'\n' {
                        out.push(next as char);
                    }
                    i += 2;
                } else {
                    i += 1;
                }
            }
            _ => {
                out.push(b as char);
                i += 1;
            }
        }
    }
    (out, i)
}

/// Parse `fn <name>` starting at `start` (after skipping whitespace already
/// done by the caller). Returns `None` if `start` is not `fn `.
fn parse_fn_name(src: &str, start: usize) -> Option<String> {
    let rest = &src[start..];
    let rest = rest.strip_prefix("fn ")?;
    let name: String = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    if name.is_empty() { None } else { Some(name) }
}

/// Scan every `.rs` file directly inside `dir` (non-recursive, matching
/// `tests/live/`'s flat layout), module-prefixing each by its file stem
/// except for `main.rs` / `mod.rs` (which only declare `mod` lines).
pub fn scan_modules_dir(dir: &Path) -> Result<Vec<GatedTest>> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .with_context(|| format!("reading directory {}", dir.display()))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("rs"))
        .collect();
    files.sort();

    let mut out = Vec::new();
    for path in files {
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_owned();
        let module_prefix = if stem == "main" || stem == "mod" {
            None
        } else {
            Some(stem)
        };
        let src = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?
            .replace("\r\n", "\n");
        out.extend(scan_source(&src, module_prefix.as_deref()));
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Targets — one per (package, `--test` binary) pair.
// ---------------------------------------------------------------------------

pub struct SweepTarget {
    pub package: String,
    pub test_name: String,
    pub gated: Vec<GatedTest>,
}

/// Resolve the fixed set of live-test targets under `workspace_root`:
/// `ff-rdp-cli`'s consolidated `tests/live/` tree (one binary, many modules),
/// plus each of `ff-rdp-core`'s standalone `tests/live_*.rs` binaries other
/// than the fixture-recording tool (see module doc comment).
pub fn default_targets(workspace_root: &Path) -> Result<Vec<SweepTarget>> {
    let mut targets = Vec::new();

    let cli_live_dir = workspace_root.join("crates/ff-rdp-cli/tests/live");
    let cli_gated = scan_modules_dir(&cli_live_dir)
        .with_context(|| format!("scanning {}", cli_live_dir.display()))?;
    targets.push(SweepTarget {
        package: "ff-rdp-cli".to_owned(),
        test_name: "live".to_owned(),
        gated: cli_gated,
    });

    let core_tests_dir = workspace_root.join("crates/ff-rdp-core/tests");
    let mut core_files: Vec<PathBuf> = std::fs::read_dir(&core_tests_dir)
        .with_context(|| format!("reading directory {}", core_tests_dir.display()))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("rs"))
        .collect();
    core_files.sort();

    for path in core_files {
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_owned();
        if stem == "live_record_fixtures" {
            continue;
        }
        if !stem.starts_with("live_") {
            continue;
        }
        let src = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?
            .replace("\r\n", "\n");
        let gated = scan_source(&src, None);
        if gated.is_empty() {
            continue;
        }
        targets.push(SweepTarget {
            package: "ff-rdp-core".to_owned(),
            test_name: stem,
            gated,
        });
    }

    Ok(targets)
}

// ---------------------------------------------------------------------------
// Env gates + partitioning
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct EnvGates {
    pub live: bool,
    pub network: bool,
    /// Something is listening on [`PREEXISTING_PORT`] (iter-158 Theme F).
    pub preexisting_available: bool,
}

impl EnvGates {
    pub fn from_process_env() -> Self {
        EnvGates {
            live: std::env::var("FF_RDP_LIVE_TESTS").as_deref() == Ok("1"),
            network: std::env::var("FF_RDP_LIVE_NETWORK_TESTS").as_deref() == Ok("1"),
            preexisting_available: preexisting_instance_available(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Partition {
    /// Every env var the test needs is set (and, if it needs a pre-existing
    /// Firefox, one is listening) — it will run for real.
    pub qualified: Vec<String>,
    /// At least one required env var is unset — it will report `ignored`.
    pub unqualified: Vec<String>,
    /// Env-qualified, but the test needs a Firefox on [`PREEXISTING_PORT`]
    /// that nobody started. Run without `--include-ignored` so libtest reports
    /// it `ignored` instead of letting it fail on `ConnectionRefused` — and
    /// counted separately from `executed`, which it never was (iter-158
    /// Theme F).
    pub preexisting: Vec<String>,
}

impl Partition {
    /// The names that must NOT get `--include-ignored`, so libtest reports
    /// them `ignored`.
    pub fn not_running(&self) -> Vec<String> {
        let mut v = self.unqualified.clone();
        v.extend(self.preexisting.iter().cloned());
        v.sort();
        v
    }
}

pub fn partition(tests: &[GatedTest], gates: &EnvGates) -> Partition {
    let mut p = Partition::default();
    for t in tests {
        let ok_live = !t.needs_live || gates.live;
        let ok_network = !t.needs_network || gates.network;
        if !(ok_live && ok_network) {
            // An unmet env gate is reported first: it is the reason the user
            // can fix by exporting a variable, and it keeps `skipped` meaning
            // exactly what iter-155 made it mean.
            p.unqualified.push(t.full_name.clone());
        } else if t.needs_preexisting && !gates.preexisting_available {
            p.preexisting.push(t.full_name.clone());
        } else {
            p.qualified.push(t.full_name.clone());
        }
    }
    p.qualified.sort();
    p.unqualified.sort();
    p.preexisting.sort();
    p
}

/// Theme B: the machine-readable count of tests that actually reached
/// Firefox (`executed`) versus those that were gate-skipped (`skipped`) —
/// known before `cargo test` runs, never inferred from its prose.
///
/// iter-158 Theme F adds the third tier: `preexisting` counts tests whose env
/// gates are met but which need a Firefox on port 6000 that nobody started.
/// Folding those into `executed` (the pre-158 behaviour) overstated what the
/// sweep had actually exercised.
/// iter-173 adds the two **unmet-precondition** tiers. Both were previously
/// reported as failing tests, which is the same lie iter-155 was filed about
/// with the sign flipped — red for a reason that has nothing to do with the
/// code under test, in the one artifact every iteration pastes into its PR
/// body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SweepSummary {
    pub executed: usize,
    pub skipped: usize,
    pub preexisting: usize,
    /// Qualified at classification time, but the port-6000 Firefox was gone by
    /// the time the tier actually ran (iter-173 Theme B). Never counted as
    /// `executed` — the test did not reach a browser — and never as a failure.
    pub vanished: usize,
    /// Ran, but panicked because Firefox never opened its debug port within
    /// the per-test launch budget under sweep load (iter-173, folded in from
    /// iter-170). Moved out of `executed` because it never reached a browser;
    /// still counted as a red so the sweep's exit status is not weakened.
    pub launch_timeout: usize,
    /// Qualified, but the phase they were in was killed by the watchdog
    /// before libtest reported a verdict for them (iter-197). Moved out of
    /// `executed` for the same reason `vanished` is — no verdict was ever
    /// produced, so counting them as exercised would overstate the sweep —
    /// but, unlike `vanished`, always a red: a phase that had to be killed is
    /// a fact about this repository, not an unmet precondition.
    pub timed_out: usize,
}

impl SweepSummary {
    pub fn total(&self) -> usize {
        self.executed
            + self.skipped
            + self.preexisting
            + self.vanished
            + self.launch_timeout
            + self.timed_out
    }
}

/// Classification-time summary: the two runtime tiers are necessarily `0`
/// here, since nothing has run yet.
pub fn summarize(part: &Partition) -> SweepSummary {
    SweepSummary {
        executed: part.qualified.len(),
        skipped: part.unqualified.len(),
        preexisting: part.preexisting.len(),
        vanished: 0,
        launch_timeout: 0,
        timed_out: 0,
    }
}

// ---------------------------------------------------------------------------
// iter-173 — runtime re-classification
// ---------------------------------------------------------------------------

/// Re-partition one target against a **fresh** probe of [`PREEXISTING_PORT`],
/// taken immediately before that target runs rather than once for the whole
/// sweep. Returns the partition to actually drive `cargo test` with, plus the
/// names that moved out of `qualified` because the browser went away.
///
/// Why: `EnvGates::from_process_env` probes port 6000 exactly once, at
/// classification time. The `ff-rdp-cli` tier then runs for 35-40 minutes
/// before the `ff-rdp-core` tier starts, and the core tests never launch a
/// browser — they connect to whatever is on 6000. In iteration 168's sweep the
/// browser was killed inside that window and all seven core tests were
/// reported `FAILED` with `ConnectionRefused`. They are an unmet precondition,
/// not a regression; re-running them against a fresh browser passed 7/7.
///
/// `probe_now == true` (or a target that needs no pre-existing instance) is a
/// no-op returning the original partition, so the common case costs one TCP
/// connect per target.
pub fn repartition_for_probe(
    gated: &[GatedTest],
    gates: &EnvGates,
    probe_now: bool,
) -> (Partition, Vec<String>) {
    if probe_now || !gates.preexisting_available {
        // Nothing changed since classification (or the browser was already
        // absent, in which case `partition` has already bucketed these).
        return (partition(gated, gates), Vec::new());
    }
    let gone = EnvGates {
        preexisting_available: false,
        ..*gates
    };
    let repart = partition(gated, &gone);
    let vanished = repart.preexisting.clone();
    (repart, vanished)
}

/// Why a `cargo test` phase reported one or more `FAILED` tests.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FailureVerdict {
    /// Failed, and the port-6000 browser they depend on is gone — an unmet
    /// precondition discovered mid-tier.
    pub vanished: Vec<String>,
    /// Failed because Firefox never opened its debug port in time.
    pub launch_timeout: Vec<String>,
    /// Everything else: a real assertion or product error.
    pub genuine: Vec<String>,
}

impl FailureVerdict {
    /// Did anything fail for a reason that is actually about the code under
    /// test?
    pub fn has_genuine_failures(&self) -> bool {
        !self.genuine.is_empty()
    }
}

/// Markers that identify a launch-timeout panic in a captured libtest failure
/// block. Both come from `crates/ff-rdp-cli/tests/common/mod.rs`'s bounded
/// port wait (iter-113 Theme A); the env-var name is included because a future
/// launcher may reword the prose but will still name the knob to raise.
const LAUNCH_TIMEOUT_MARKERS: &[&str] =
    &["never opened debug port", "FF_RDP_LIVE_LAUNCH_TIMEOUT_SECS"];

/// Attribute each `FAILED` test in one phase's libtest output to a cause.
///
/// `browser_still_up` is a **fresh** probe of [`PREEXISTING_PORT`] taken after
/// the phase finished, and only means anything for a target whose tests need
/// that browser (`target_needs_preexisting`) — which is why it is passed in
/// rather than probed here: this function stays pure and unit-testable without
/// a Firefox anywhere.
///
/// A launch timeout is attributed before a vanished browser: it names its own
/// cause explicitly in the panic message, so it should not be swept into the
/// weaker inference.
pub fn classify_failures(
    stdout: &str,
    browser_still_up: bool,
    target_needs_preexisting: bool,
) -> FailureVerdict {
    let mut verdict = FailureVerdict::default();
    let browser_gone = target_needs_preexisting && !browser_still_up;
    for (name, body) in failure_blocks(stdout) {
        if LAUNCH_TIMEOUT_MARKERS.iter().any(|m| body.contains(m)) {
            verdict.launch_timeout.push(name);
        } else if browser_gone {
            verdict.vanished.push(name);
        } else {
            verdict.genuine.push(name);
        }
    }
    // A phase can fail without any per-test block (a compile error, a harness
    // panic before the first test). Those names never appear above, and the
    // caller must keep treating such a phase as failed — see `run()`.
    verdict.vanished.sort();
    verdict.launch_timeout.sort();
    verdict.genuine.sort();
    verdict
}

/// Split libtest's `failures:` detail section into `(test name, captured
/// output)` pairs.
///
/// libtest prints each failing test's captured output under a
/// `---- <name> stdout ----` header, then repeats the bare names in a
/// `failures:` list. We key off the headers: they carry the panic message,
/// which is what distinguishes a launch timeout from an assertion.
fn failure_blocks(stdout: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut current: Option<(String, String)> = None;
    for line in stdout.lines() {
        if let Some(name) = failure_block_header(line) {
            if let Some(block) = current.take() {
                out.push(block);
            }
            current = Some((name, String::new()));
        } else if line.starts_with("----") || line.trim() == "failures:" {
            // Any other `----` rule, or the trailing bare-name list, ends the
            // block we were accumulating.
            if let Some(block) = current.take() {
                out.push(block);
            }
        } else if let Some((_, body)) = current.as_mut() {
            body.push_str(line);
            body.push('\n');
        }
    }
    if let Some(block) = current.take() {
        out.push(block);
    }
    out
}

/// `---- some::test stdout ----` → `Some("some::test")`.
fn failure_block_header(line: &str) -> Option<String> {
    let rest = line.strip_prefix("---- ")?;
    let name = rest.strip_suffix(" stdout ----")?;
    if name.is_empty() {
        None
    } else {
        Some(name.to_owned())
    }
}

// ---------------------------------------------------------------------------
// Command building
// ---------------------------------------------------------------------------

/// Build the `cargo test -p <package> --test <test_name> -- [--include-ignored
/// --test-threads=<jobs>] --exact <names…>` command for one phase, or `None`
/// when `names` is empty (nothing to run for this phase).
///
/// `jobs` is only emitted for the real run (`include_ignored`): phase 2's
/// tests are selected precisely so libtest reports them `ignored` without
/// executing, and a thread count there would describe nothing.
pub fn phase_command(
    package: &str,
    test_name: &str,
    names: &[String],
    include_ignored: bool,
    jobs: usize,
) -> Option<Command> {
    if names.is_empty() {
        return None;
    }
    let mut cmd = Command::new("cargo");
    cmd.arg("test");
    cmd.args(["-p", package, "--test", test_name]);
    cmd.arg("--");
    if include_ignored {
        cmd.arg("--include-ignored");
        cmd.arg(format!("--test-threads={}", jobs.max(1)));
    }
    cmd.arg("--exact");
    cmd.args(names);
    Some(cmd)
}

/// Why the watchdog stopped a phase (iter-197).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhaseTimeout {
    /// The bound that was exceeded — whichever of [`PhaseBounds`]' two
    /// windows applied at the time.
    pub bound: std::time::Duration,
    /// `true` when libtest had not printed a single line yet, i.e. the phase
    /// died in `cargo`'s build window rather than mid-tier.
    pub before_first_output: bool,
}

/// One `cargo test` phase's result: libtest's exit status plus its stdout,
/// which has already been echoed through to ours line by line.
struct PhaseOutcome {
    success: bool,
    stdout: String,
    /// `Some` when the watchdog killed the phase instead of libtest finishing
    /// it. `success` is always `false` then.
    timed_out: Option<PhaseTimeout>,
}

/// Put the phase's `cargo` in a process group of its own so the watchdog has
/// something to kill that includes the test binary.
///
/// **This is the whole reason a group exists here.** Killing the `cargo test`
/// process alone accomplishes nothing: the hung party is the *test binary*
/// cargo spawned, which cargo does not reap on its own death — it would keep
/// running, keep holding whatever it had launched, and keep the sweep's
/// problem exactly where it was.
///
/// Trade-off, stated because it is real: a `cargo` in its own group is no
/// longer in the terminal's foreground group, so an operator's Ctrl-C reaches
/// `xtask` but not `cargo`. That is the same hazard the run-wide rule "never
/// kill a sweep mid-run" already warns about, and the watchdog is precisely
/// what makes reaching for Ctrl-C unnecessary. If you do interrupt a sweep by
/// hand, sweep up after it with `pgrep -f ff-rdp-profile-` (the same signal
/// [`managed_firefox_pids`] uses).
#[cfg(unix)]
fn own_process_group(cmd: &mut Command) {
    use std::os::unix::process::CommandExt as _;
    cmd.process_group(0);
}

/// No-op on non-Unix: Windows has no process groups in this sense, and
/// [`kill_phase_tree`] uses `taskkill /T` there instead.
#[cfg(not(unix))]
fn own_process_group(_cmd: &mut Command) {}

/// Kill a timed-out phase and everything `cargo` started under it.
///
/// Unix: signal the negative pgid, which is the group [`own_process_group`]
/// created (pgid == the child's pid), so `cargo` *and* the test binary go
/// together. `kill(1)` is used rather than `libc::kill` because the workspace
/// forbids `unsafe` and one process spawn on the timeout path costs nothing.
///
/// Note what this deliberately does **not** reach: a Firefox `ff-rdp launch`
/// started is put into a process group of *its own*
/// (`commands::launch::build_command`, iter-95 Theme A), specifically so that
/// group signals do not travel between them. Reaping those is
/// [`reap_managed_firefox`]'s job, and the caller must run it.
fn kill_phase_tree(child: &mut std::process::Child) {
    let pid = child.id();
    #[cfg(unix)]
    let mut killer = {
        let mut c = Command::new("kill");
        c.args(["-KILL", &format!("-{pid}")]);
        c
    };
    #[cfg(not(unix))]
    let mut killer = {
        let mut c = Command::new("taskkill");
        c.args(["/F", "/T", "/PID", &pid.to_string()]);
        c
    };
    let _ = killer
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    // Belt and braces: if the group signal did not land (no `kill` on PATH, a
    // platform without groups), at least the direct child goes away.
    let _ = child.kill();
}

/// Run one phase, streaming its stdout to ours as it arrives **and** keeping a
/// copy for [`classify_failures`], under the watchdog `bounds` describe.
///
/// Streaming matters: the CLI tier runs for 35-40 minutes and a plain
/// `output()` would show the operator nothing until it finished. stderr is
/// inherited untouched (cargo's build progress lives there); libtest reprints
/// each failing test's captured output on stdout, which is the part we parse.
///
/// iter-197: the read loop used to be a plain blocking `read_line`, which is
/// why a single hung test hung the sweep forever — libtest has no per-test
/// timeout of any kind, so nothing anywhere in the pipeline had a bound. The
/// read now happens on a helper thread feeding a channel, and the main thread
/// waits on it with `recv_timeout`, so *silence* is what the bound is measured
/// against. That choice matters: a wall-clock bound on the whole phase would
/// have to be sized for a 40-minute tier and would therefore be useless, while
/// silence between libtest result lines is bounded by one test's duration no
/// matter how long the tier is. See [`DEFAULT_PHASE_STALL_SECS`].
fn run_phase(cmd: &mut Command, what: &str, bounds: PhaseBounds) -> Result<PhaseOutcome> {
    use std::io::{BufRead, BufReader, Write};
    use std::sync::mpsc::{RecvTimeoutError, channel};

    cmd.stdout(std::process::Stdio::piped());
    own_process_group(cmd);
    let mut child = cmd
        .spawn()
        .with_context(|| format!("failed to spawn {what}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("no stdout pipe for {what}"))?;

    let (tx, rx) = channel::<String>();
    let pump = std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        loop {
            line.clear();
            // A read error is treated as EOF: the phase is over either way,
            // and the exit status below is the authority on how it ended.
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    if tx.send(std::mem::take(&mut line)).is_err() {
                        break;
                    }
                }
            }
        }
    });

    let mut captured = String::new();
    let mut seen_output = false;
    let mut timed_out: Option<PhaseTimeout> = None;

    let absorb = |line: &str, captured: &mut String| {
        print!("{line}");
        let _ = std::io::stdout().flush();
        captured.push_str(line);
    };

    loop {
        let Some(bound) = bounds.current(seen_output) else {
            // Watchdog disabled for this window — block exactly as pre-197.
            match rx.recv() {
                Ok(line) => {
                    seen_output = true;
                    absorb(&line, &mut captured);
                    continue;
                }
                Err(_) => break,
            }
        };
        match rx.recv_timeout(bound) {
            Ok(line) => {
                seen_output = true;
                absorb(&line, &mut captured);
            }
            Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {
                eprintln!(
                    "live-sweep: WATCHDOG — {what} produced no output for {}s ({}); killing its \
                     process group. libtest has no per-test timeout, so without this the sweep \
                     would wait forever (iter-197).",
                    bound.as_secs(),
                    if seen_output {
                        "--phase-stall-secs, measured between libtest result lines"
                    } else {
                        "--phase-build-secs, measured before libtest's first line"
                    }
                );
                timed_out = Some(PhaseTimeout {
                    bound,
                    before_first_output: !seen_output,
                });
                kill_phase_tree(&mut child);
                break;
            }
        }
    }

    let status = child
        .wait()
        .with_context(|| format!("waiting for {what}"))?;
    // The pump exits on EOF, which the kill above guarantees.
    let _ = pump.join();
    // Anything the pump had already queued when we stopped listening is still
    // part of this phase's output, and `unreported_tests` reads it.
    while let Ok(line) = rx.try_recv() {
        absorb(&line, &mut captured);
    }

    Ok(PhaseOutcome {
        success: timed_out.is_none() && status.success(),
        stdout: captured,
        timed_out,
    })
}

// ---------------------------------------------------------------------------
// iter-197 — naming what hung, and reaping what it left behind
// ---------------------------------------------------------------------------

/// The prefix of libtest's output that contains its live result lines, i.e.
/// everything before the `failures:` section.
///
/// Everything after that marker is *replayed* material: each failing test's
/// captured stdout, reprinted verbatim under a `---- <name> stdout ----`
/// header, followed by the bare-name list. A live test's own output can — and
/// in this repository does — contain lines that look exactly like libtest
/// result lines, so a parser that reads the whole stream can be told that a
/// test reported when it never did. Restricting to `known` is not enough on
/// its own: one live test printing another's name in a passing verdict would
/// erase that test from the `timed_out` tier, which is the same "counts a test
/// that did not run" lie iter-155 exists to prevent, one layer down.
///
/// A watchdog-killed phase never reaches the marker, so this is the identity
/// function on exactly the input that matters most here.
fn libtest_result_section(stdout: &str) -> &str {
    match stdout.find("\nfailures:\n") {
        // +1 keeps the newline that terminates the last result line.
        Some(idx) => &stdout[..=idx],
        None => stdout,
    }
}

/// The names libtest has already reported a verdict for, restricted to
/// `known` — the exact list this phase was handed via `--exact` — and to the
/// live-results prefix of the stream ([`libtest_result_section`]).
///
/// Both restrictions are load-bearing; see that function's doc for why the
/// `known` filter alone is not sufficient.
fn reported_test_names<'a>(stdout: &str, known: &'a [String]) -> Vec<&'a str> {
    let mut out = Vec::new();
    for line in libtest_result_section(stdout).lines() {
        let Some(rest) = line.strip_prefix("test ") else {
            continue;
        };
        // `test <name> ... ok` / `... FAILED` / `... ignored, <reason>`.
        // `rfind` rather than `find`: nothing in a libtest name contains
        // " ... ", but a trailing ignore reason may.
        let Some(idx) = rest.rfind(" ... ") else {
            continue;
        };
        let name = &rest[..idx];
        if let Some(hit) = known.iter().find(|k| k.as_str() == name) {
            out.push(hit.as_str());
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// The tests this phase was asked to run that libtest never reported a verdict
/// for — i.e. the ones the watchdog's kill cut short.
///
/// This is how a timed-out phase names the culprit without inventing one.
/// libtest prints a result line only when a test *finishes*, so with `--jobs
/// N` up to N names can be in flight; the observed hang left exactly one (276
/// of 277 CLI-tier tests had reported). The set is exact — it is derived from
/// the `--exact` list the phase was given, not from prose.
pub fn unreported_tests(qualified: &[String], stdout: &str) -> Vec<String> {
    let reported = reported_test_names(stdout, qualified);
    qualified
        .iter()
        .filter(|n| !reported.contains(&n.as_str()))
        .cloned()
        .collect()
}

/// Names libtest itself flagged with `test <name> has been running for over N
/// seconds`, restricted to `known` for the same reason as
/// [`reported_test_names`].
///
/// libtest's own 60-second notice is the strongest available hint about *which*
/// of the in-flight tests is the stuck one — it is the only line the hung sweep
/// of 2026-08-23 produced. It is a hint, not the verdict: a legitimately slow
/// test triggers it too, which is why the reported failure names the unreported
/// set and mentions these separately.
pub fn slow_flagged_tests(stdout: &str, known: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for line in libtest_result_section(stdout).lines() {
        let Some(rest) = line.strip_prefix("test ") else {
            continue;
        };
        let Some(idx) = rest.find(" has been running for over ") else {
            continue;
        };
        let name = &rest[..idx];
        if let Some(hit) = known.iter().find(|k| k.as_str() == name) {
            out.push(hit.clone());
        }
    }
    out.sort();
    out.dedup();
    out
}

/// How many test names a watchdog report prints before summarising the rest.
///
/// The set is normally tiny — libtest prints a result line as each test
/// *finishes*, so at `--jobs N` at most N tests can be in flight, and the
/// observed hang left exactly one. It is not bounded by that in the pathological
/// case (a very small `--phase-stall-secs`, or a hang in the first seconds of a
/// tier), and a 243-name single-line report is not a report — it is a wall.
/// The full set is still what the accounting uses; only the printed form is
/// capped.
const REPORTED_NAME_CAP: usize = 20;

/// Render a list of test names for an operator: all of them when there are few,
/// the first [`REPORTED_NAME_CAP`] plus a count when there are many.
pub fn format_name_list(names: &[String]) -> String {
    if names.is_empty() {
        return "<none>".to_owned();
    }
    if names.len() <= REPORTED_NAME_CAP {
        return names.join(", ");
    }
    format!(
        "{}, … and {} more",
        names[..REPORTED_NAME_CAP].join(", "),
        names.len() - REPORTED_NAME_CAP
    )
}

/// Substring that identifies a Firefox `ff-rdp` started for itself: every
/// managed profile directory is named `ff-rdp-profile-<16 chars>` and is
/// passed to Firefox as `--profile <dir>` (`commands::launch::build_command`).
///
/// Matching the command line rather than a marker file on disk is deliberate:
/// several live tests point `$FF_RDP_HOME` at a per-test temp directory, so
/// their profiles are not under the default root at all and a root scan would
/// miss exactly the instances a hang is most likely to strand.
const MANAGED_PROFILE_ARG_MARKER: &str = "ff-rdp-profile-";

/// Pick the ff-rdp-managed Firefox processes out of a `<pid> <command line>`
/// process listing.
///
/// Pure so the matching rules are testable without a browser anywhere. Both
/// conditions are required: the command line must name a managed profile
/// **and** be a Firefox. `self_pid` is excluded so the sweep can never kill
/// itself — the failure mode the iteration plan calls out as "the checker that
/// matches itself", which is real here because `xtask`'s own command line is
/// in the same listing.
pub fn managed_firefox_pids(listing: &str, self_pid: u32) -> Vec<u32> {
    let mut out = Vec::new();
    for line in listing.lines() {
        let Some((pid_text, cmdline)) = line.trim_start().split_once(char::is_whitespace) else {
            continue;
        };
        let Ok(pid) = pid_text.parse::<u32>() else {
            continue;
        };
        if pid == self_pid {
            continue;
        }
        if !cmdline.contains(MANAGED_PROFILE_ARG_MARKER) {
            continue;
        }
        if !cmdline.to_ascii_lowercase().contains("firefox") {
            continue;
        }
        out.push(pid);
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// Snapshot every process as `<pid> <command line>` lines, or `None` if the
/// platform's process lister could not be run.
fn process_listing() -> Option<String> {
    #[cfg(unix)]
    let mut cmd = {
        let mut c = Command::new("ps");
        c.args(["-eo", "pid=,args="]);
        c
    };
    #[cfg(not(unix))]
    let mut cmd = {
        let mut c = Command::new("powershell");
        c.args([
            "-NoProfile",
            "-Command",
            "Get-CimInstance Win32_Process | ForEach-Object { \
             \"$($_.ProcessId) $($_.CommandLine)\" }",
        ]);
        c
    };
    let out = cmd.stderr(std::process::Stdio::null()).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// SIGKILL (or `taskkill /F`) one pid, ignoring the outcome.
fn kill_pid_hard(pid: u32) {
    #[cfg(unix)]
    let mut cmd = {
        let mut c = Command::new("kill");
        c.args(["-KILL", &pid.to_string()]);
        c
    };
    #[cfg(not(unix))]
    let mut cmd = {
        let mut c = Command::new("taskkill");
        c.args(["/F", "/PID", &pid.to_string()]);
        c
    };
    let _ = cmd
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

/// Kill every ff-rdp-managed Firefox still running, and return the pids.
///
/// Called only after the watchdog has killed a phase. The kill signal that
/// stops the phase cannot reach these: `ff-rdp launch` puts each Firefox in a
/// process group of its own on purpose (iter-95 Theme A), so they survive the
/// group kill and — because their parent `ff-rdp` is long gone — are reparented
/// to init and outlive the sweep entirely. That is the "four Firefox processes
/// still open" half of the 2026-08-23 hang, and this is what closes it.
///
/// Only ff-rdp's *own* ephemeral profiles are matched, so a browser the
/// operator started by hand (including the port-6000 instance the
/// `preexisting` tier depends on) is never touched.
pub fn reap_managed_firefox() -> Vec<u32> {
    let Some(listing) = process_listing() else {
        return Vec::new();
    };
    let pids = managed_firefox_pids(&listing, std::process::id());
    for pid in &pids {
        kill_pid_hard(*pid);
    }
    pids
}

// ---------------------------------------------------------------------------
// run()
// ---------------------------------------------------------------------------

pub fn run(args: Args) -> Result<()> {
    let targets = default_targets(&args.workspace_root)?;
    let gates = EnvGates::from_process_env();
    let bounds = PhaseBounds::from_args(&args);

    let total_gated: usize = targets.iter().map(|t| t.gated.len()).sum();
    if total_gated == 0 {
        return Err(anyhow!(
            "live-sweep: found 0 gated live tests under {} — the scanner or \
             workspace-root configuration is almost certainly broken (there \
             are normally several dozen); refusing to report a false-empty \
             sweep rather than silently doing nothing, which is exactly the \
             failure class this tool exists to prevent",
            args.workspace_root.display()
        ));
    }

    if !gates.preexisting_available {
        eprintln!(
            "live-sweep: nothing is listening on 127.0.0.1:{PREEXISTING_PORT} — tests that \
             connect to a Firefox they did not launch will be reported `preexisting` and run \
             WITHOUT --include-ignored (start one with \
             `firefox --start-debugger-server {PREEXISTING_PORT} --headless` to execute them)"
        );
    }

    let mut totals = SweepSummary::default();
    let mut overall_ok = true;

    for target in &targets {
        let needs_preexisting = target.gated.iter().any(|g| g.needs_preexisting);

        // iter-173 Theme B: re-probe port 6000 immediately before this target,
        // not once for the whole sweep. `default_targets` puts `ff-rdp-cli`
        // first and it runs for 35-40 minutes; the browser the `ff-rdp-core`
        // tier connects to may not have survived it.
        let probe_now = if needs_preexisting && !args.dry_run && gates.preexisting_available {
            preexisting_instance_available()
        } else {
            gates.preexisting_available
        };
        let (part, vanished_before_tier) = repartition_for_probe(&target.gated, &gates, probe_now);
        if !vanished_before_tier.is_empty() {
            eprintln!(
                "live-sweep: the Firefox on 127.0.0.1:{PREEXISTING_PORT} was there at \
                 classification time and is gone now — {} test(s) in -p {} --test {} will report \
                 `ignored` (unmet precondition) rather than failing on ConnectionRefused",
                vanished_before_tier.len(),
                target.package,
                target.test_name
            );
        }

        let summary = summarize(&part);
        totals.skipped += summary.skipped;
        // A vanished browser leaves the tests in `part.preexisting`; split
        // that count back out so `preexisting` keeps meaning "nobody had
        // started one when the sweep began".
        totals.preexisting += summary.preexisting - vanished_before_tier.len();
        totals.vanished += vanished_before_tier.len();
        let mut executed = summary.executed;

        // Computed once so the number this prints and the number the real
        // run below actually passes to libtest cannot drift apart.
        let jobs = jobs_for_target(needs_preexisting, args.jobs);

        eprintln!(
            "live-sweep: -p {} --test {}: {} qualified (will run for real at \
             --test-threads={jobs}), {} will report `ignored` (env gate), {} will report \
             `ignored` (no Firefox on {PREEXISTING_PORT})",
            target.package,
            target.test_name,
            summary.executed,
            summary.skipped,
            summary.preexisting
        );

        if args.dry_run {
            totals.executed += executed;
            continue;
        }

        if let Some(mut cmd) = phase_command(
            &target.package,
            &target.test_name,
            &part.qualified,
            true,
            jobs,
        ) {
            let what = format!(
                "`cargo test -p {} --test {}` (phase 1: real run, --test-threads={jobs})",
                target.package, target.test_name
            );
            let outcome = run_phase(&mut cmd, &what, bounds)?;
            if let Some(timeout) = outcome.timed_out {
                // iter-197. The phase was killed, so libtest never printed a
                // `failures:` section and `classify_failures` has nothing to
                // read. What *is* exact is which of the `--exact` names never
                // got a verdict line — that set is the culprit, and it is
                // derived from the sweep's own input rather than from prose.
                let unreported = unreported_tests(&part.qualified, &outcome.stdout);
                let flagged = slow_flagged_tests(&outcome.stdout, &part.qualified);
                eprintln!(
                    "live-sweep: -p {} --test {} was KILLED after {}s of silence ({}). {} \
                     test(s) never reported a verdict and are counted `timed_out`, not \
                     `executed`: {}",
                    target.package,
                    target.test_name,
                    timeout.bound.as_secs(),
                    if timeout.before_first_output {
                        "before libtest's first line — the build, not a test"
                    } else {
                        "mid-tier"
                    },
                    unreported.len(),
                    if unreported.is_empty() {
                        "<none — every test reported; the phase hung after the last one>".to_owned()
                    } else {
                        format_name_list(&unreported)
                    }
                );
                if !flagged.is_empty() {
                    eprintln!(
                        "live-sweep: libtest's own slow-test notice named: {}",
                        format_name_list(&flagged)
                    );
                }

                // The kill above cannot reach the browsers: `ff-rdp launch`
                // puts each Firefox in its own process group. Reap them by
                // command line, or they outlive the sweep and poison the next
                // one (iter-146's postmortem, iter-188's abandoned run 3).
                let reaped = reap_managed_firefox();
                if reaped.is_empty() {
                    eprintln!("live-sweep: no ff-rdp-managed Firefox was left behind");
                } else {
                    eprintln!(
                        "live-sweep: reaped {} orphaned ff-rdp-managed Firefox process(es): {}",
                        reaped.len(),
                        reaped
                            .iter()
                            .map(u32::to_string)
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }

                executed = executed.saturating_sub(unreported.len());
                totals.timed_out += unreported.len();
                overall_ok = false;
            } else if !outcome.success {
                // A failing phase is only forgiven for causes that are not
                // about the code under test, and only when libtest actually
                // named the tests — a compile error or a harness panic
                // produces no failure blocks and must still fail the sweep.
                let browser_still_up = if needs_preexisting {
                    preexisting_instance_available()
                } else {
                    true
                };
                let verdict =
                    classify_failures(&outcome.stdout, browser_still_up, needs_preexisting);
                let attributed =
                    verdict.vanished.len() + verdict.launch_timeout.len() + verdict.genuine.len();

                if !verdict.vanished.is_empty() {
                    eprintln!(
                        "live-sweep: the Firefox on 127.0.0.1:{PREEXISTING_PORT} disappeared \
                         during -p {} --test {} — counting {} test(s) as an unmet precondition, \
                         NOT as failures: {}",
                        target.package,
                        target.test_name,
                        verdict.vanished.len(),
                        verdict.vanished.join(", ")
                    );
                }
                if !verdict.launch_timeout.is_empty() {
                    eprintln!(
                        "live-sweep: {} test(s) in -p {} --test {} failed because Firefox never \
                         opened its debug port within the launch budget (raise \
                         FF_RDP_LIVE_LAUNCH_TIMEOUT_SECS); the machine could not start a \
                         browser in time — this is not a product failure: {}",
                        verdict.launch_timeout.len(),
                        target.package,
                        target.test_name,
                        verdict.launch_timeout.join(", ")
                    );
                }

                executed =
                    executed.saturating_sub(verdict.vanished.len() + verdict.launch_timeout.len());
                totals.vanished += verdict.vanished.len();
                totals.launch_timeout += verdict.launch_timeout.len();

                // The sweep still exits non-zero for a launch timeout: it is a
                // red libtest result, and turning reds green on inference is
                // how a real regression gets waved through (iter-155). Only a
                // vanished browser — whose tests never ran at all — is
                // forgiven, and only when every failure is accounted for.
                if verdict.has_genuine_failures()
                    || !verdict.launch_timeout.is_empty()
                    || attributed == 0
                {
                    overall_ok = false;
                }
            }
        }

        if let Some(mut cmd) = phase_command(
            &target.package,
            &target.test_name,
            &part.not_running(),
            false,
            1,
        ) {
            let what = format!(
                "`cargo test -p {} --test {}` (phase 2: report ignored)",
                target.package, target.test_name
            );
            // Phase 2 executes nothing, so a stall here is never a slow test —
            // it is a wedged cargo. It runs under the same bounds anyway:
            // `success` is already `false` for a timed-out phase, which is the
            // only thing this branch consumes.
            let outcome = run_phase(&mut cmd, &what, bounds)?;
            overall_ok &= outcome.success;
        }

        totals.executed += executed;
    }

    let SweepSummary {
        executed,
        skipped,
        preexisting,
        vanished,
        launch_timeout,
        timed_out,
    } = totals;
    let grand_total = totals.total();
    println!(
        "LIVE_SWEEP_SUMMARY executed={executed} skipped={skipped} preexisting={preexisting} \
         vanished={vanished} launch_timeout={launch_timeout} timed_out={timed_out} \
         total={grand_total}"
    );

    if overall_ok {
        Ok(())
    } else if timed_out > 0 {
        Err(anyhow!(
            "live-sweep: a phase had to be killed by the watchdog — {timed_out} qualified live \
             test(s) never reported a verdict (named above), plus any ordinary failures. A \
             timed-out phase is always a red: raise --phase-stall-secs only with evidence that \
             the tier legitimately went quiet for longer than the bound."
        ))
    } else {
        Err(anyhow!(
            "live-sweep: one or more qualified live tests failed — see cargo test output above"
        ))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // scan_source
    // -----------------------------------------------------------------------

    #[test]
    fn scan_source_classifies_live_only_reason() {
        let src = r#"
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_dom_text_longstring_roundtrip() {
    if std::env::var("FF_RDP_LIVE_TESTS").is_err() {
        return;
    }
}
"#;
        let got = scan_source(src, Some("live_102_longstring_and_reload"));
        assert_eq!(got.len(), 1);
        assert_eq!(
            got[0].full_name,
            "live_102_longstring_and_reload::live_dom_text_longstring_roundtrip"
        );
        assert!(got[0].needs_live);
        assert!(!got[0].needs_network);
    }

    #[test]
    fn scan_source_classifies_network_only_reason() {
        let src = r#"
#[test]
#[ignore = "requires Firefox, network access, and FF_RDP_LIVE_NETWORK_TESTS=1"]
fn live_network_security_info_https() {
    if std::env::var("FF_RDP_LIVE_NETWORK_TESTS").is_err() {
        return;
    }
}
"#;
        let got = scan_source(src, Some("live_104_security_pwa"));
        assert_eq!(got.len(), 1);
        assert!(!got[0].needs_live);
        assert!(got[0].needs_network);
    }

    #[test]
    fn scan_source_classifies_both_required() {
        let src = r#"
#[test]
#[ignore = "requires Firefox + network access — set FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1"]
fn live_throttle_slow3g_slows_fetch() {
    if !live_tests_enabled() || !live_network_tests_enabled() {
        return;
    }
}
"#;
        let got = scan_source(src, Some("live_109_throttle_block"));
        assert_eq!(got.len(), 1);
        assert!(got[0].needs_live);
        assert!(got[0].needs_network);
        assert_eq!(
            got[0].full_name,
            "live_109_throttle_block::live_throttle_slow3g_slows_fetch"
        );
    }

    /// Regression fixture: mirrors `live_daemon_watch_targets.rs`'s real
    /// multi-line `#[ignore = "…\` reason (backslash line-continuation).
    #[test]
    fn scan_source_handles_multiline_reason() {
        let src = "#[test]\n#[ignore = \"requires Firefox and FF_RDP_LIVE_TESTS=1; KNOWN FAILING pending \\\n            iteration-101 Theme A (watchTargets re-engagement) — see doc comment\"]\nfn live_daemon_watch_targets() {}\n";
        let got = scan_source(src, Some("live_daemon_watch_targets"));
        assert_eq!(
            got.len(),
            1,
            "expected exactly one gated test, got: {got:?}"
        );
        assert!(got[0].needs_live);
        assert!(!got[0].needs_network);
        assert_eq!(
            got[0].full_name,
            "live_daemon_watch_targets::live_daemon_watch_targets"
        );
    }

    #[test]
    fn scan_source_handles_intervening_cfg_attribute() {
        let src =
            "#[test]\n#[ignore = \"requires FF_RDP_LIVE_TESTS=1\"]\n#[cfg(unix)]\nfn t() {}\n";
        let got = scan_source(src, None);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].full_name, "t");
    }

    #[test]
    fn scan_source_ignores_unrelated_ignore_attrs() {
        let src = r#"
#[test]
#[ignore = "may perform a real network download depending on cache state"]
fn unrelated() {}
"#;
        let got = scan_source(src, None);
        assert!(
            got.is_empty(),
            "an #[ignore] reason with no FF_RDP_LIVE mention must not be classified as a gated live test"
        );
    }

    #[test]
    fn scan_source_no_module_prefix_when_none() {
        let src = "#[test]\n#[ignore = \"FF_RDP_LIVE_TESTS=1\"]\nfn live_handshake() {}\n";
        let got = scan_source(src, None);
        assert_eq!(got[0].full_name, "live_handshake");
    }

    // -----------------------------------------------------------------------
    // partition / summarize — AC2 (executed count)
    // -----------------------------------------------------------------------

    fn gated(name: &str, needs_live: bool, needs_network: bool) -> GatedTest {
        GatedTest {
            full_name: name.to_owned(),
            needs_live,
            needs_network,
            needs_preexisting: false,
        }
    }

    /// `test_155_executed_count_is_reported`: the executed count is `0` when
    /// the relevant env gates are unset, and rises deterministically as gates
    /// are set — computed from classification alone, before any `cargo test`
    /// process runs.
    #[test]
    fn test_155_executed_count_is_reported() {
        let tests = vec![
            gated("a::t1", true, false),
            gated("a::t2", true, true),
            gated("a::t3", false, true),
        ];

        let none_set = EnvGates {
            live: false,
            network: false,
            preexisting_available: true,
        };
        let summary = summarize(&partition(&tests, &none_set));
        assert_eq!(
            summary.executed, 0,
            "with both env gates unset, executed must be 0"
        );
        assert_eq!(summary.skipped, 3);
        assert_eq!(summary.total(), 3);

        let live_only = EnvGates {
            live: true,
            network: false,
            preexisting_available: true,
        };
        let summary = summarize(&partition(&tests, &live_only));
        assert_eq!(
            summary.executed, 1,
            "only the live-only test qualifies with FF_RDP_LIVE_TESTS=1 alone"
        );
        assert_eq!(summary.skipped, 2);

        let both_set = EnvGates {
            live: true,
            network: true,
            preexisting_available: true,
        };
        let summary = summarize(&partition(&tests, &both_set));
        assert_eq!(summary.executed, 3);
        assert_eq!(summary.skipped, 0);
    }

    #[test]
    fn partition_sorts_names_deterministically() {
        let tests = vec![gated("z", false, false), gated("a", false, false)];
        let gates = EnvGates {
            live: true,
            network: true,
            preexisting_available: true,
        };
        let part = partition(&tests, &gates);
        assert_eq!(part.qualified, vec!["a".to_owned(), "z".to_owned()]);
    }

    // -----------------------------------------------------------------------
    // iter-158 Theme F — the `preexisting` tier
    // -----------------------------------------------------------------------

    fn gated_preexisting(name: &str) -> GatedTest {
        GatedTest {
            full_name: name.to_owned(),
            needs_live: true,
            needs_network: false,
            needs_preexisting: true,
        }
    }

    /// A file whose live tests resolve their port through `firefox_port()` (or
    /// spell out `--start-debugger-server 6000`) needs an instance somebody
    /// else started.
    #[test]
    fn test_158_source_markers_identify_preexisting_suites() {
        assert!(source_needs_preexisting_instance(
            "use support::recording::{firefox_port, should_run_live};"
        ));
        assert!(source_needs_preexisting_instance(
            "#[ignore = \"… start Firefox with --start-debugger-server 6000\"]"
        ));
        assert!(source_needs_preexisting_instance(
            "//! requires a running headless Firefox with the remote debugger enabled on port 6000"
        ));
        assert!(
            !source_needs_preexisting_instance("let ff = LiveFirefox::headless_on_random_port();"),
            "a suite that launches its own Firefox is not `preexisting`"
        );
    }

    /// AC `live_158_sweep_reports_three_tiers` (classification half): with
    /// nothing on 127.0.0.1:6000 the preexisting tests leave `executed` and
    /// land in their own bucket; with an instance available they are executed
    /// like any other qualified test.
    #[test]
    fn test_158_preexisting_tier_is_split_out_of_executed() {
        let tests = vec![
            gated("cli::t1", true, false),
            gated_preexisting("core_a"),
            gated_preexisting("core_b"),
        ];

        let no_instance = EnvGates {
            live: true,
            network: true,
            preexisting_available: false,
        };
        let part = partition(&tests, &no_instance);
        let summary = summarize(&part);
        assert_eq!(summary.executed, 1, "only the self-launching test executes");
        assert_eq!(summary.preexisting, 2);
        assert_eq!(summary.skipped, 0);
        assert_eq!(summary.total(), 3);
        assert_eq!(
            part.not_running(),
            vec!["core_a".to_owned(), "core_b".to_owned()],
            "preexisting tests must run WITHOUT --include-ignored so libtest reports \
             them `ignored` instead of failing on ConnectionRefused"
        );

        let with_instance = EnvGates {
            preexisting_available: true,
            ..no_instance
        };
        let summary = summarize(&partition(&tests, &with_instance));
        assert_eq!(summary.executed, 3);
        assert_eq!(summary.preexisting, 0);
    }

    /// An unmet env gate is reported as `skipped`, not `preexisting` — a user
    /// can fix the former by exporting a variable, and `skipped` keeps the
    /// meaning iter-155 gave it.
    #[test]
    fn test_158_env_gate_takes_precedence_over_preexisting() {
        let tests = vec![gated_preexisting("core_a")];
        let gates = EnvGates {
            live: false,
            network: false,
            preexisting_available: false,
        };
        let summary = summarize(&partition(&tests, &gates));
        assert_eq!(summary.skipped, 1);
        assert_eq!(summary.preexisting, 0);
    }

    /// The real `ff-rdp-core` live targets must classify as `preexisting` —
    /// they connect to a Firefox they never launch. Six of the seven failures
    /// in the 2026-08-13 sweep were exactly this.
    #[test]
    fn test_158_real_core_targets_are_preexisting() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(std::path::Path::parent)
            .expect("workspace root")
            .to_path_buf();
        let Ok(targets) = default_targets(&root) else {
            eprintln!("test_158_real_core_targets_are_preexisting: workspace not readable");
            return;
        };
        let core_preexisting: usize = targets
            .iter()
            .filter(|t| t.package == "ff-rdp-core")
            .flat_map(|t| &t.gated)
            .filter(|g| g.needs_preexisting)
            .count();
        let core_total: usize = targets
            .iter()
            .filter(|t| t.package == "ff-rdp-core")
            .map(|t| t.gated.len())
            .sum();
        assert!(core_total > 0, "expected some ff-rdp-core live targets");
        assert_eq!(
            core_preexisting, core_total,
            "every ff-rdp-core live test connects to a pre-existing Firefox on port 6000"
        );

        let cli_preexisting: usize = targets
            .iter()
            .filter(|t| t.package == "ff-rdp-cli")
            .flat_map(|t| &t.gated)
            .filter(|g| g.needs_preexisting)
            .count();
        assert_eq!(
            cli_preexisting, 0,
            "the ff-rdp-cli live suites launch their own Firefox"
        );
    }

    // -----------------------------------------------------------------------
    // iter-173 Task D — a self-launching suite is never `preexisting`
    // -----------------------------------------------------------------------

    /// AC4: an `ff-rdp-cli` live source that *names* `firefox_port` (the field
    /// in `daemon.<port>.json`) but launches its own Firefox must classify as
    /// executed, not `preexisting`. On `main` the bare-substring marker put it
    /// in the wrong bucket, so with nothing on port 6000 it would be reported
    /// `ignored` instead of run — iter-155's false green by another road.
    #[test]
    fn test_173_registry_assertion_does_not_make_a_suite_preexisting() {
        let src = r#"
use crate::common::LiveFirefox;

#[test]
#[ignore = "requires Firefox and FF_RDP_LIVE_TESTS=1"]
fn live_172_published_record_is_complete() {
    let ff = LiveFirefox::headless_on_random_port();
    let rec: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(rec["firefox_port"].as_u64(), Some(u64::from(ff.port())));
}
"#;
        assert!(
            !source_needs_preexisting_instance(src),
            "a suite that launches its own Firefox must not be reclassified as \
             `preexisting` merely because it asserts on the `firefox_port` field"
        );
        let got = scan_source(src, Some("live_172_registry"));
        assert_eq!(got.len(), 1);
        assert!(
            !got[0].needs_preexisting,
            "the scanned test must carry the corrected classification"
        );
    }

    /// The negative marker must not swallow the genuine `preexisting` signal:
    /// a source that only reads `firefox_port()` and never launches anything
    /// still needs somebody else's browser.
    #[test]
    fn test_173_self_launch_marker_does_not_weaken_the_preexisting_tier() {
        assert!(source_needs_preexisting_instance(
            "use support::recording::{firefox_port, should_run_live};\nfn t() { firefox_port(); }"
        ));
    }

    // -----------------------------------------------------------------------
    // iter-173 Theme B — a vanished browser is a precondition, not a failure
    // -----------------------------------------------------------------------

    /// AC2 (classification half): the port-6000 probe said "available" when
    /// the sweep started, the CLI tier ran for 35 minutes, and a fresh probe
    /// before the core tier says the browser is gone. Those tests must leave
    /// `qualified` and land in `not_running()` so libtest reports them
    /// `ignored` — never `FAILED` on `ConnectionRefused`.
    #[test]
    fn test_173_vanished_browser_moves_core_tests_out_of_qualified() {
        let tests = vec![gated_preexisting("live_connect_and_list_tabs")];
        let gates = EnvGates {
            live: true,
            network: true,
            preexisting_available: true,
        };

        let (still_there, none_gone) = repartition_for_probe(&tests, &gates, true);
        assert!(none_gone.is_empty());
        assert_eq!(still_there.qualified, vec!["live_connect_and_list_tabs"]);

        let (after, vanished) = repartition_for_probe(&tests, &gates, false);
        assert_eq!(
            vanished,
            vec!["live_connect_and_list_tabs".to_owned()],
            "a browser present at classification time and gone at tier time \
             must be reported as an unmet precondition"
        );
        assert!(after.qualified.is_empty(), "it must not run for real");
        assert_eq!(after.not_running(), vec!["live_connect_and_list_tabs"]);
    }

    /// A target that launches its own Firefox is untouched by the re-probe —
    /// the fresh probe result is irrelevant to it.
    #[test]
    fn test_173_reprobe_does_not_touch_self_launching_targets() {
        let tests = vec![gated("cli::t1", true, false)];
        let gates = EnvGates {
            live: true,
            network: false,
            preexisting_available: true,
        };
        let (part, vanished) = repartition_for_probe(&tests, &gates, false);
        assert!(vanished.is_empty());
        assert_eq!(part.qualified, vec!["cli::t1"]);
    }

    // -----------------------------------------------------------------------
    // iter-173 — post-hoc failure attribution
    // -----------------------------------------------------------------------

    /// The real shape of iteration 168's core tier: every test `FAILED` with
    /// `ConnectionRefused` because the browser was gone. With a fresh probe
    /// confirming it, none of them is a genuine failure.
    const CONNECTION_REFUSED_OUTPUT: &str = "\
running 2 tests
test live_connect_and_list_tabs ... FAILED
test live_selected_tab_is_marked ... FAILED

failures:

---- live_connect_and_list_tabs stdout ----
thread 'live_connect_and_list_tabs' panicked at crates/ff-rdp-core/tests/live_firefox_test.rs:41:
connect failed: ConnectionFailed(Os { code: 61, kind: ConnectionRefused, message: \"Connection refused\" })

---- live_selected_tab_is_marked stdout ----
thread 'live_selected_tab_is_marked' panicked at crates/ff-rdp-core/tests/live_firefox_test.rs:77:
connect failed: ConnectionFailed(Os { code: 61, kind: ConnectionRefused, message: \"Connection refused\" })

failures:
    live_connect_and_list_tabs
    live_selected_tab_is_marked

test result: FAILED. 0 passed; 2 failed; 0 ignored
";

    #[test]
    fn test_173_connection_refused_after_browser_loss_is_not_a_genuine_failure() {
        let verdict = classify_failures(CONNECTION_REFUSED_OUTPUT, false, true);
        assert_eq!(
            verdict.vanished,
            vec![
                "live_connect_and_list_tabs".to_owned(),
                "live_selected_tab_is_marked".to_owned()
            ]
        );
        assert!(verdict.genuine.is_empty());
        assert!(!verdict.has_genuine_failures());
    }

    /// The same output with the browser still up is a real failure — the
    /// forgiveness is conditional on evidence, not on the error text.
    #[test]
    fn test_173_connection_refused_with_the_browser_up_is_a_genuine_failure() {
        let verdict = classify_failures(CONNECTION_REFUSED_OUTPUT, true, true);
        assert_eq!(verdict.genuine.len(), 2);
        assert!(verdict.vanished.is_empty());
        assert!(verdict.has_genuine_failures());
    }

    /// Folded in from iter-170: a 30 s Firefox-launch budget spent against a
    /// fully loaded machine is an unmet precondition, and must be told apart
    /// from a product failure — even though it still fails the sweep.
    #[test]
    fn test_173_launch_timeout_is_classified_separately_from_a_real_failure() {
        let stdout = "\
running 2 tests
test live_123_daemon_autostart_and_registry::live_daemon_autostart_tabless ... FAILED
test live_140_eval::live_eval_returns_number ... FAILED

failures:

---- live_123_daemon_autostart_and_registry::live_daemon_autostart_tabless stdout ----
thread 'main' panicked at crates/ff-rdp-cli/tests/common/mod.rs:1008:
RawFirefox: /Applications/Firefox.app/Contents/MacOS/firefox (pid 43844) never opened debug port 64638 within 30s (raise FF_RDP_LIVE_LAUNCH_TIMEOUT_SECS)

---- live_140_eval::live_eval_returns_number stdout ----
thread 'main' panicked at crates/ff-rdp-cli/tests/live/live_140_eval.rs:22:
assertion `left == right` failed
  left: 3
 right: 4

failures:
    live_123_daemon_autostart_and_registry::live_daemon_autostart_tabless
    live_140_eval::live_eval_returns_number

test result: FAILED. 0 passed; 2 failed; 0 ignored
";
        let verdict = classify_failures(stdout, true, false);
        assert_eq!(
            verdict.launch_timeout,
            vec![
                "live_123_daemon_autostart_and_registry::live_daemon_autostart_tabless".to_owned()
            ]
        );
        assert_eq!(
            verdict.genuine,
            vec!["live_140_eval::live_eval_returns_number".to_owned()],
            "a real assertion failure must stay a real failure"
        );
        assert!(verdict.has_genuine_failures());
    }

    /// A launch timeout names its own cause, so it wins over the weaker
    /// "the browser is gone" inference even on a preexisting-tier target.
    #[test]
    fn test_173_launch_timeout_wins_over_the_vanished_inference() {
        let stdout = "\
failures:

---- t stdout ----
never opened debug port 6000 within 30s

failures:
    t
";
        let verdict = classify_failures(stdout, false, true);
        assert_eq!(verdict.launch_timeout, vec!["t".to_owned()]);
        assert!(verdict.vanished.is_empty());
    }

    /// A phase that fails without naming a single test (a compile error, or a
    /// harness panic before the first test) attributes nothing — `run()` uses
    /// that to keep failing the sweep rather than forgiving a blank verdict.
    #[test]
    fn test_173_a_phase_with_no_failure_blocks_attributes_nothing() {
        let verdict = classify_failures("error[E0433]: failed to resolve\n", false, true);
        assert_eq!(verdict, FailureVerdict::default());
        assert!(!verdict.has_genuine_failures());
    }

    /// AC3: the accounting stays conserved — the two new tiers are carved out
    /// of `executed`, never added on top of it.
    #[test]
    fn test_173_summary_total_conserves_every_tier() {
        let s = SweepSummary {
            executed: 10,
            skipped: 3,
            preexisting: 2,
            vanished: 7,
            launch_timeout: 1,
            timed_out: 4,
        };
        assert_eq!(s.total(), 27);
        let all_executed = SweepSummary {
            executed: 22,
            skipped: 3,
            preexisting: 2,
            vanished: 0,
            launch_timeout: 0,
            timed_out: 0,
        };
        assert_eq!(
            all_executed.total(),
            s.total(),
            "moving tests between tiers must not change the total — that is what \
             makes `executed` impossible to inflate"
        );
    }

    // -----------------------------------------------------------------------
    // phase_command
    // -----------------------------------------------------------------------

    fn arg_strings(cmd: &Command) -> Vec<String> {
        cmd.get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn phase_command_none_when_names_empty() {
        assert!(phase_command("ff-rdp-cli", "live", &[], true, 6).is_none());
        assert!(phase_command("ff-rdp-cli", "live", &[], false, 1).is_none());
    }

    #[test]
    fn phase_command_omits_include_ignored_when_false() {
        let cmd = phase_command("ff-rdp-cli", "live", &["a::b".to_owned()], false, 1).unwrap();
        let args = arg_strings(&cmd);
        assert!(!args.iter().any(|a| a == "--include-ignored"));
        assert!(args.iter().any(|a| a == "--exact"));
        assert!(args.iter().any(|a| a == "a::b"));
    }

    #[test]
    fn phase_command_includes_include_ignored_when_true() {
        let cmd = phase_command("ff-rdp-cli", "live", &["a::b".to_owned()], true, 1).unwrap();
        let args = arg_strings(&cmd);
        assert!(args.iter().any(|a| a == "--include-ignored"));
    }

    // -----------------------------------------------------------------------
    // iter-188 Theme C — concurrency
    // -----------------------------------------------------------------------

    /// The real run carries the requested thread count, so the sweep is no
    /// longer pinned to one test at a time.
    #[test]
    fn test_188_phase_one_carries_the_requested_thread_count() {
        let cmd = phase_command("ff-rdp-cli", "live", &["a::b".to_owned()], true, 6).unwrap();
        let args = arg_strings(&cmd);
        assert!(
            args.iter().any(|a| a == "--test-threads=6"),
            "phase 1 must pass the requested concurrency; got {args:?}"
        );
        assert!(
            !args.iter().any(|a| a == "--test-threads=1"),
            "the pre-188 hard-coded serial flag must be gone; got {args:?}"
        );
    }

    /// `--jobs 1` reproduces the pre-188 serial sweep exactly.
    #[test]
    fn test_188_jobs_one_reproduces_the_serial_sweep() {
        let cmd = phase_command("ff-rdp-cli", "live", &["a::b".to_owned()], true, 1).unwrap();
        assert!(arg_strings(&cmd).iter().any(|a| a == "--test-threads=1"));
    }

    /// Phase 2 never executes anything, so it must not carry a thread count
    /// at all — a concurrency for tests that report `ignored` describes
    /// nothing and would only invite the reader to believe they ran.
    #[test]
    fn test_188_phase_two_carries_no_thread_count() {
        let cmd = phase_command("ff-rdp-cli", "live", &["a::b".to_owned()], false, 6).unwrap();
        assert!(
            !arg_strings(&cmd)
                .iter()
                .any(|a| a.starts_with("--test-threads")),
            "phase 2 must not pass --test-threads"
        );
    }

    /// A zero (or absent) job count can never disable libtest's own default
    /// by emitting `--test-threads=0`, which libtest rejects.
    #[test]
    fn test_188_zero_jobs_is_clamped_to_one() {
        let cmd = phase_command("ff-rdp-cli", "live", &["a::b".to_owned()], true, 0).unwrap();
        assert!(arg_strings(&cmd).iter().any(|a| a == "--test-threads=1"));
    }

    /// Targets that depend on the port-6000 Firefox stay serial regardless of
    /// `--jobs`: they share one browser they did not start.
    #[test]
    fn test_188_preexisting_targets_stay_serial() {
        assert_eq!(jobs_for_target(true, 6), 1);
        assert_eq!(jobs_for_target(true, 1), 1);
        assert_eq!(jobs_for_target(false, 6), 6);
        assert_eq!(jobs_for_target(false, 0), 1);
    }

    /// The default never exceeds the measured knee, and never drops below 1
    /// however odd the machine looks.
    #[test]
    fn test_188_default_jobs_is_capped_by_the_measured_knee() {
        let jobs = default_jobs();
        assert!(
            (1..=MAX_SWEEP_JOBS).contains(&jobs),
            "default concurrency {jobs} must be in 1..={MAX_SWEEP_JOBS}"
        );
        let cores = std::thread::available_parallelism()
            .map(std::num::NonZeroUsize::get)
            .unwrap_or(1);
        assert!(
            jobs <= cores,
            "default concurrency {jobs} must not oversubscribe {cores} core(s)"
        );
    }

    // -----------------------------------------------------------------------
    // default_targets — real tree regression fence
    // -----------------------------------------------------------------------

    /// The real tree must yield a healthy population of gated tests — a
    /// regression fence against exactly the failure this tool exists to
    /// prevent (a scanner that silently finds nothing and reports a
    /// false-empty, all-green sweep).
    #[test]
    fn real_tree_yields_many_gated_tests() {
        let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..");
        if !workspace_root.join("crates/ff-rdp-cli/tests/live").is_dir() {
            eprintln!("real_tree_yields_many_gated_tests: tests/live not found — skipping");
            return;
        }
        let targets = default_targets(&workspace_root).expect("default_targets");
        let total: usize = targets.iter().map(|t| t.gated.len()).sum();
        assert!(
            total > 50,
            "expected several dozen gated live tests across all targets, got {total}"
        );

        // The exact test named in the plan's own dogfood_path must be found
        // and correctly classified as network-gated.
        let cli_target = targets
            .iter()
            .find(|t| t.package == "ff-rdp-cli")
            .expect("ff-rdp-cli target must exist");
        let live_109 = cli_target
            .gated
            .iter()
            .find(|t| t.full_name == "live_109_throttle_block::live_block_url_pattern")
            .expect("live_109_throttle_block::live_block_url_pattern must be discovered");
        assert!(live_109.needs_network);
    }

    #[test]
    fn run_errors_on_empty_scan_rather_than_reporting_false_empty_sweep() {
        let tmp = tempfile::TempDir::new().unwrap();
        // Empty crates/ layout: no ff-rdp-cli/tests/live, no ff-rdp-core/tests.
        std::fs::create_dir_all(tmp.path().join("crates/ff-rdp-cli/tests/live")).unwrap();
        std::fs::create_dir_all(tmp.path().join("crates/ff-rdp-core/tests")).unwrap();
        let args = Args {
            workspace_root: tmp.path().to_path_buf(),
            dry_run: true,
            jobs: 1,
            phase_stall_secs: DEFAULT_PHASE_STALL_SECS,
            phase_build_secs: DEFAULT_PHASE_BUILD_SECS,
        };
        let err = run(args).unwrap_err();
        assert!(
            err.to_string().contains("found 0 gated live tests"),
            "expected the empty-scan guard to fire, got: {err}"
        );
    }

    // -----------------------------------------------------------------------
    // test_155_skipped_live_test_is_not_counted_passed (AC1) — integration
    // -----------------------------------------------------------------------

    /// `test_155_skipped_live_test_is_not_counted_passed`: classify the real
    /// `live_109_throttle_block::live_block_url_pattern` test (named in the
    /// plan's own dogfood_path), partition it as unqualified (network gate
    /// unset), build the resulting phase-2 command (no `--include-ignored`),
    /// and actually run it — asserting from libtest's own summary output
    /// that the test is reported `ignored`, never `ok`. This proves the fix
    /// at the mechanism the defect lives in, not by reading the source.
    #[test]
    fn test_155_skipped_live_test_is_not_counted_passed() {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let workspace_root = manifest_dir.join("..").join("..");
        let cli_live_dir = workspace_root.join("crates/ff-rdp-cli/tests/live");
        if !cli_live_dir.is_dir() {
            eprintln!(
                "test_155_skipped_live_test_is_not_counted_passed: tests/live not found — skipping"
            );
            return;
        }

        let gated = scan_modules_dir(&cli_live_dir).expect("scan_modules_dir");
        let target_name = "live_109_throttle_block::live_block_url_pattern";
        let target = gated
            .iter()
            .find(|t| t.full_name == target_name)
            .unwrap_or_else(|| panic!("{target_name} must be discovered by the scanner"));
        assert!(
            target.needs_network,
            "{target_name} must be classified as network-gated"
        );

        let gates = EnvGates {
            live: true,
            network: false,
            preexisting_available: true,
        };
        let part = partition(&gated, &gates);
        assert!(
            part.unqualified.contains(&target.full_name),
            "with FF_RDP_LIVE_NETWORK_TESTS unset, {target_name} must be unqualified"
        );

        let mut cmd = phase_command(
            "ff-rdp-cli",
            "live",
            std::slice::from_ref(&target.full_name),
            false,
            1,
        )
        .expect("phase command for a non-empty name list");
        cmd.env_remove("FF_RDP_LIVE_TESTS");
        cmd.env_remove("FF_RDP_LIVE_NETWORK_TESTS");
        cmd.current_dir(&workspace_root);
        let output = cmd
            .output()
            .expect("failed to run `cargo test -p ff-rdp-cli --test live` subprocess");
        let stdout = String::from_utf8_lossy(&output.stdout);

        assert!(
            stdout.contains(&format!("test {target_name} ... ignored")),
            "expected libtest to report {target_name} as `ignored` \
             (env gate unset, --include-ignored absent); stdout:\n{stdout}"
        );
        assert!(
            !stdout.contains(&format!("test {target_name} ... ok")),
            "the unqualified test must never be reported `ok` — that is exactly \
             the iter-155 defect (a skipped live test reporting green); stdout:\n{stdout}"
        );
    }

    // -----------------------------------------------------------------------
    // iter-197 — the phase watchdog
    // -----------------------------------------------------------------------

    use std::time::Duration;

    /// A child that prints one line and then never says anything again — the
    /// shape of the 2026-08-23 hang (libtest reported 276 of 277 tests and
    /// then went silent forever).
    fn prints_then_hangs(text: &str) -> Command {
        if cfg!(windows) {
            let mut c = Command::new("cmd");
            c.args(["/c", &format!("echo {text} & ping -n 600 127.0.0.1 > nul")]);
            c
        } else {
            let mut c = Command::new("/bin/sh");
            c.args(["-c", &format!("printf '%s\n' '{text}'; sleep 600")]);
            c
        }
    }

    /// A child that prints one line and exits 0 straight away.
    fn prints_and_exits(text: &str) -> Command {
        if cfg!(windows) {
            let mut c = Command::new("cmd");
            c.args(["/c", &format!("echo {text}")]);
            c
        } else {
            let mut c = Command::new("/bin/sh");
            c.args(["-c", &format!("printf '%s\n' '{text}'")]);
            c
        }
    }

    /// AC1, mechanism half: a phase that goes silent is killed at the stated
    /// bound rather than waited on forever, and says which bound fired.
    #[test]
    fn iter_197_watchdog_kills_a_silent_phase_at_the_stall_bound() {
        let mut cmd = prints_then_hangs("running 1 test");
        let bounds = PhaseBounds {
            build: Duration::from_secs(30),
            stall: Duration::from_secs(1),
        };
        let started = std::time::Instant::now();
        let outcome = run_phase(&mut cmd, "silent phase", bounds).expect("run_phase");
        let elapsed = started.elapsed();

        let timeout = outcome
            .timed_out
            .expect("a phase that never speaks again must time out, not hang");
        assert_eq!(timeout.bound, Duration::from_secs(1));
        assert!(
            !timeout.before_first_output,
            "it printed a line first, so the *stall* bound is the one that fired"
        );
        assert!(!outcome.success, "a killed phase is never a success");
        assert!(
            outcome.stdout.contains("running 1 test"),
            "output seen before the kill must still be captured: {:?}",
            outcome.stdout
        );
        assert!(
            elapsed < Duration::from_secs(30),
            "the watchdog must return at its own bound, not the build bound; took {elapsed:?}"
        );
    }

    /// The build bound is the one that fires when nothing has been printed at
    /// all — the `cargo` window, where a silent minute is normal and a silent
    /// hour is a wedged target-directory lock.
    #[test]
    fn iter_197_watchdog_attributes_a_silent_build_to_the_build_bound() {
        let mut cmd = if cfg!(windows) {
            let mut c = Command::new("cmd");
            c.args(["/c", "ping -n 600 127.0.0.1 > nul"]);
            c
        } else {
            let mut c = Command::new("/bin/sh");
            c.args(["-c", "sleep 600"]);
            c
        };
        let bounds = PhaseBounds {
            build: Duration::from_secs(1),
            stall: Duration::from_secs(30),
        };
        let outcome = run_phase(&mut cmd, "silent build", bounds).expect("run_phase");
        let timeout = outcome.timed_out.expect("must time out");
        assert!(timeout.before_first_output);
        assert_eq!(timeout.bound, Duration::from_secs(1));
    }

    /// The watchdog must not change what a normal phase does: a command that
    /// finishes is reported by its exit status, with `timed_out` unset.
    #[test]
    fn iter_197_watchdog_leaves_a_finishing_phase_alone() {
        let mut cmd = prints_and_exits("test result: ok. 1 passed");
        let outcome =
            run_phase(&mut cmd, "normal phase", PhaseBounds::default()).expect("run_phase");
        assert!(outcome.timed_out.is_none());
        assert!(outcome.success);
        assert!(outcome.stdout.contains("test result: ok. 1 passed"));
    }

    /// `--phase-stall-secs 0` / `--phase-build-secs 0` restore the pre-197
    /// unbounded wait. Exercised on a command that *finishes*, since the whole
    /// point of the disabled path is that it would otherwise never return.
    #[test]
    fn iter_197_zero_bounds_disable_the_watchdog() {
        let bounds = PhaseBounds {
            build: Duration::ZERO,
            stall: Duration::ZERO,
        };
        assert_eq!(bounds.current(false), None);
        assert_eq!(bounds.current(true), None);
        let mut cmd = prints_and_exits("hello");
        let outcome = run_phase(&mut cmd, "unbounded phase", bounds).expect("run_phase");
        assert!(outcome.timed_out.is_none());
        assert!(outcome.success);
    }

    /// The bound that applies switches from `build` to `stall` the moment
    /// libtest says anything.
    #[test]
    fn iter_197_bounds_switch_window_on_first_output() {
        let bounds = PhaseBounds::default();
        assert_eq!(
            bounds.current(false),
            Some(Duration::from_secs(DEFAULT_PHASE_BUILD_SECS))
        );
        assert_eq!(
            bounds.current(true),
            Some(Duration::from_secs(DEFAULT_PHASE_STALL_SECS))
        );
    }

    /// The kill must reach what `cargo` spawned, not just `cargo`.
    ///
    /// This is the reason [`own_process_group`] exists: the hung party is the
    /// test binary, and killing its parent leaves it running. The stand-in
    /// here is a shell that backgrounds a long sleep and reports its pid; the
    /// assertion is that the pid is gone after the watchdog fires.
    ///
    /// Unix-only: the Windows path is `taskkill /T`, which walks a parent-pid
    /// tree rather than a process group and cannot be exercised by the same
    /// fixture.
    #[cfg(unix)]
    #[test]
    fn iter_197_watchdog_kill_reaches_the_grandchild() {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let pidfile = tmp.path().join("grandchild.pid");
        let script = format!(
            "/bin/sh -c 'echo $$ > {pid}; sleep 600' & printf 'running 1 test\n'; wait",
            pid = pidfile.display()
        );
        let mut cmd = Command::new("/bin/sh");
        cmd.args(["-c", &script]);
        let bounds = PhaseBounds {
            build: Duration::from_secs(30),
            stall: Duration::from_secs(1),
        };
        let outcome = run_phase(&mut cmd, "grandchild phase", bounds).expect("run_phase");
        assert!(
            outcome.timed_out.is_some(),
            "the phase must have been killed"
        );

        let recorded = std::fs::read_to_string(&pidfile).expect("grandchild wrote its pid");
        let pid: u32 = recorded.trim().parse().expect("a pid");

        // Poll: the signal is asynchronous, and this is the same bounded-wait
        // shape the live harness uses after a SIGKILL (iter-168).
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut alive = true;
        while std::time::Instant::now() < deadline {
            let gone = !Command::new("kill")
                .args(["-0", &pid.to_string()])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            if gone {
                alive = false;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(
            !alive,
            "killing only the direct child would leave pid {pid} running — that is \
             the whole reason the phase gets a process group of its own"
        );
    }

    // -----------------------------------------------------------------------
    // iter-197 — naming the culprit
    // -----------------------------------------------------------------------

    /// AC1, reporting half: given the exact `--exact` list and the truncated
    /// output a killed phase leaves, the sweep names the test that never
    /// reported — and only that one.
    #[test]
    fn iter_197_unreported_tests_names_the_test_without_a_verdict() {
        let qualified = vec![
            "live_a::one".to_owned(),
            "live_b::two".to_owned(),
            "live_158_launch_lifecycle::live_158_launch_survives_contended_bind".to_owned(),
        ];
        let stdout = "\nrunning 3 tests\n\
             test live_a::one ... ok\n\
             test live_b::two ... ok\n\
             test live_158_launch_lifecycle::live_158_launch_survives_contended_bind has been \
             running for over 60 seconds\n";
        assert_eq!(
            unreported_tests(&qualified, stdout),
            vec!["live_158_launch_lifecycle::live_158_launch_survives_contended_bind".to_owned()],
            "276-of-277 is exactly the shape of the observed hang"
        );
    }

    /// A phase that finishes leaves nothing unreported — the accounting must
    /// not manufacture a `timed_out` tier out of a healthy run.
    #[test]
    fn iter_197_unreported_tests_is_empty_when_every_test_reported() {
        let qualified = vec!["live_a::one".to_owned(), "live_b::two".to_owned()];
        let stdout = "test live_a::one ... ok\ntest live_b::two ... FAILED\n";
        assert!(unreported_tests(&qualified, stdout).is_empty());
    }

    /// A failing test's captured stdout is reprinted verbatim by libtest and
    /// may itself contain `test <something> ... ok`. Only names the sweep
    /// asked for are ever accepted, so such a line can neither invent a
    /// verdict nor invent a name.
    #[test]
    fn iter_197_reported_names_ignore_prose_from_captured_output() {
        let qualified = vec!["live_a::one".to_owned(), "live_b::two".to_owned()];
        let stdout = "test live_a::one ... FAILED\n\
             \n\
             failures:\n\
             \n\
             ---- live_a::one stdout ----\n\
             test live_b::two ... ok\n";
        assert_eq!(
            unreported_tests(&qualified, stdout),
            vec!["live_b::two".to_owned()],
            "`live_b::two` never ran; the line naming it came out of another test's \
             captured output"
        );
    }

    /// libtest's own 60-second notice is surfaced as a hint, and is likewise
    /// restricted to names the sweep asked for.
    #[test]
    fn iter_197_slow_flagged_tests_reads_libtests_own_notice() {
        let qualified = vec!["live_a::one".to_owned(), "live_b::two".to_owned()];
        let stdout = "test live_a::one has been running for over 60 seconds\n\
             test unknown::other has been running for over 60 seconds\n";
        assert_eq!(
            slow_flagged_tests(stdout, &qualified),
            vec!["live_a::one".to_owned()]
        );
    }

    // -----------------------------------------------------------------------
    // iter-197 — reaping what the kill cannot reach
    // -----------------------------------------------------------------------

    /// AC3: the reaper matches ff-rdp's own ephemeral profiles and nothing
    /// else — not a browser the operator started, not the sweep itself.
    #[test]
    fn iter_197_managed_firefox_pids_matches_only_ff_rdp_managed_browsers() {
        let listing = "\
  501 /Applications/Firefox.app/Contents/MacOS/firefox -no-remote --profile /var/f/ff-rdp-profile-abcdefghij123456 --headless
  502 /Applications/Firefox.app/Contents/MacOS/firefox -P default
  503 /Users/x/ff-rdp/target/debug/xtask live-sweep --jobs 6
  504 /usr/bin/grep ff-rdp-profile-
  505 /snap/firefox/current/usr/lib/firefox/firefox --profile /tmp/ff-rdp-profile-0123456789abcdef
not-a-process-line
";
        assert_eq!(
            managed_firefox_pids(listing, 999),
            vec![501, 505],
            "a managed profile *and* a firefox binary are both required"
        );
    }

    /// The sweep must never be able to kill itself, even if its own command
    /// line happens to contain the marker (it does, whenever `--phase-*` or a
    /// path argument mentions a profile).
    #[test]
    fn iter_197_managed_firefox_pids_never_matches_the_caller() {
        let listing = "  777 /x/firefox --profile /tmp/ff-rdp-profile-aaaaaaaaaaaaaaaa\n";
        assert!(
            managed_firefox_pids(listing, 777).is_empty(),
            "excluding self is what stops the checker matching itself"
        );
    }

    /// A watchdog report must stay readable. Forcing the bound to 3 s against
    /// the real tier on 2026-08-24 left 243 names unreported and printed them
    /// as one ~20 KB line — technically complete, operationally useless.
    #[test]
    fn iter_197_name_list_is_capped_for_an_operator() {
        assert_eq!(format_name_list(&[]), "<none>");

        let few: Vec<String> = (0..3).map(|i| format!("live_x::t{i}")).collect();
        assert_eq!(format_name_list(&few), "live_x::t0, live_x::t1, live_x::t2");

        let many: Vec<String> = (0..243).map(|i| format!("live_x::t{i}")).collect();
        let rendered = format_name_list(&many);
        assert!(rendered.starts_with("live_x::t0, live_x::t1, "));
        assert!(
            rendered.ends_with("… and 223 more"),
            "the remainder must be counted, not dropped: {rendered}"
        );
        assert_eq!(
            rendered.matches(", ").count(),
            REPORTED_NAME_CAP,
            "exactly {REPORTED_NAME_CAP} names plus the summary tail"
        );
    }

    /// AC: `total` stays conserved when tests move into the new tier.
    #[test]
    fn iter_197_summary_total_conserves_the_timed_out_tier() {
        let hung = SweepSummary {
            executed: 276,
            skipped: 0,
            preexisting: 0,
            vanished: 0,
            launch_timeout: 0,
            timed_out: 1,
        };
        let clean = SweepSummary {
            executed: 277,
            ..SweepSummary::default()
        };
        assert_eq!(hung.total(), 277);
        assert_eq!(hung.total(), clean.total());
    }
}
