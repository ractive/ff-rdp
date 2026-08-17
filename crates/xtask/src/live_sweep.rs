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
}

impl SweepSummary {
    pub fn total(&self) -> usize {
        self.executed + self.skipped + self.preexisting + self.vanished + self.launch_timeout
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
/// --test-threads=1] --exact <names…>` command for one phase, or `None` when
/// `names` is empty (nothing to run for this phase).
pub fn phase_command(
    package: &str,
    test_name: &str,
    names: &[String],
    include_ignored: bool,
) -> Option<Command> {
    if names.is_empty() {
        return None;
    }
    let mut cmd = Command::new("cargo");
    cmd.arg("test");
    cmd.args(["-p", package, "--test", test_name]);
    cmd.arg("--");
    if include_ignored {
        cmd.args(["--include-ignored", "--test-threads=1"]);
    }
    cmd.arg("--exact");
    cmd.args(names);
    Some(cmd)
}

/// One `cargo test` phase's result: libtest's exit status plus its stdout,
/// which has already been echoed through to ours line by line.
struct PhaseOutcome {
    success: bool,
    stdout: String,
}

/// Run one phase, streaming its stdout to ours as it arrives **and** keeping a
/// copy for [`classify_failures`].
///
/// Streaming matters: the CLI tier runs for 35-40 minutes and a plain
/// `output()` would show the operator nothing until it finished. stderr is
/// inherited untouched (cargo's build progress lives there); libtest reprints
/// each failing test's captured output on stdout, which is the part we parse.
fn run_phase(cmd: &mut Command, what: &str) -> Result<PhaseOutcome> {
    use std::io::{BufRead, BufReader, Write};

    cmd.stdout(std::process::Stdio::piped());
    let mut child = cmd
        .spawn()
        .with_context(|| format!("failed to spawn {what}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("no stdout pipe for {what}"))?;

    let mut captured = String::new();
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader
            .read_line(&mut line)
            .with_context(|| format!("reading stdout of {what}"))?;
        if n == 0 {
            break;
        }
        print!("{line}");
        let _ = std::io::stdout().flush();
        captured.push_str(&line);
    }

    let status = child
        .wait()
        .with_context(|| format!("waiting for {what}"))?;
    Ok(PhaseOutcome {
        success: status.success(),
        stdout: captured,
    })
}

// ---------------------------------------------------------------------------
// run()
// ---------------------------------------------------------------------------

pub fn run(args: Args) -> Result<()> {
    let targets = default_targets(&args.workspace_root)?;
    let gates = EnvGates::from_process_env();

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

        eprintln!(
            "live-sweep: -p {} --test {}: {} qualified (will run for real), {} will report \
             `ignored` (env gate), {} will report `ignored` (no Firefox on {PREEXISTING_PORT})",
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

        if let Some(mut cmd) =
            phase_command(&target.package, &target.test_name, &part.qualified, true)
        {
            let what = format!(
                "`cargo test -p {} --test {}` (phase 1: real run)",
                target.package, target.test_name
            );
            let outcome = run_phase(&mut cmd, &what)?;
            if !outcome.success {
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
        ) {
            let what = format!(
                "`cargo test -p {} --test {}` (phase 2: report ignored)",
                target.package, target.test_name
            );
            let outcome = run_phase(&mut cmd, &what)?;
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
    } = totals;
    let grand_total = totals.total();
    println!(
        "LIVE_SWEEP_SUMMARY executed={executed} skipped={skipped} preexisting={preexisting} \
         vanished={vanished} launch_timeout={launch_timeout} total={grand_total}"
    );

    if overall_ok {
        Ok(())
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
    // phase_command
    // -----------------------------------------------------------------------

    #[test]
    fn phase_command_none_when_names_empty() {
        assert!(phase_command("ff-rdp-cli", "live", &[], true).is_none());
        assert!(phase_command("ff-rdp-cli", "live", &[], false).is_none());
    }

    #[test]
    fn phase_command_omits_include_ignored_when_false() {
        let cmd = phase_command("ff-rdp-cli", "live", &["a::b".to_owned()], false).unwrap();
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(!args.iter().any(|a| a == "--include-ignored"));
        assert!(args.iter().any(|a| a == "--exact"));
        assert!(args.iter().any(|a| a == "a::b"));
    }

    #[test]
    fn phase_command_includes_include_ignored_when_true() {
        let cmd = phase_command("ff-rdp-cli", "live", &["a::b".to_owned()], true).unwrap();
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(args.iter().any(|a| a == "--include-ignored"));
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
}
