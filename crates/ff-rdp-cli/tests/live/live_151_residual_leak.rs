//! Live tests for iteration 151 — a residual live-suite Firefox leak that
//! survived iteration 146.
//!
//! See `kb/iterations/iteration-151-residual-live-firefox-leak.md` for the
//! full investigation. Summary of what this file locks in:
//!
//! ## Theme A — name the leaking test
//!
//! [`live_151_leaked_profile_names_its_test`] asserts a `LiveFirefox`-spawned
//! profile carries an `.ff-rdp-owner-test` marker naming the spawning test —
//! see `tests/common/mod.rs`'s `SPAWNING_TEST_ENV` and
//! `src/util/profile_dir.rs`'s `OWNER_TEST_MARKER`/`write_owner_test_marker`.
//!
//! ## Theme B — the confirmed leak source(s)
//!
//! Two real, still-open instances of the exact "no RAII guard across an
//! assertion" bug class iteration 146 fixed in
//! `live_96_profile_cleanup.rs`'s `launch_headless`:
//!
//! 1. `live_90_daemon_lifecycle.rs` (×2), `live_daemon_stop_mdn.rs`, and
//!    `live_142_daemon_stop_pid_honesty.rs` each wrapped their `LiveFirefox`
//!    guard in `std::mem::ManuallyDrop` immediately after spawning, so
//!    `daemon stop` (or `launch --replace`) alone was responsible for
//!    killing Firefox — but every assertion between that point and the
//!    final liveness check ran with **no** guard at all. A failure in any of
//!    them (a non-zero `daemon stop`, a slow port release, a pid-honesty
//!    mismatch) panicked with Firefox still alive and nothing left to reap
//!    it. Fixed by removing the `ManuallyDrop` suppression — the guard now
//!    stays a normal binding for the rest of each function, so its `Drop`
//!    is a harmless no-op on the happy path and a real safety net on panic.
//! 2. `live_142_disk_growth.rs`'s `launch_headless` launched Firefox via a
//!    bare `Command` with no guard whatsoever and relied entirely on a
//!    *manual*, later `kill_pid` call for cleanup — this is the exact
//!    pre-146 `live_96_profile_cleanup.rs` shape, just never migrated when
//!    146 fixed that file. Fixed by adding `common::FirefoxGuard`, a small
//!    RAII wrapper over a raw PID (this file's launches need a custom
//!    `FF_RDP_HOME` env var that `common::LiveFirefox` doesn't expose, so it
//!    can't reuse that type outright).
//! 3. `launch --replace` starts a REPLACEMENT Firefox after reaping the prior
//!    instance, and a `LiveFirefox` guard owns only the PID it launched
//!    itself — so `live_86_perf_field_fixes.rs`'s
//!    `live_launch_replace_handles_stuck_prior` and
//!    `live_123_daemon_autostart_and_registry.rs`'s port-scoping test each
//!    orphaned one Firefox on *every* run, happy path included. This class
//!    was missed by the original Theme B audit (which looked only for
//!    discarded guards, not for processes nothing ever owned) and is the
//!    better arithmetic fit for the measured ~1-orphan-per-100-tests rate
//!    than the intermittent `ManuallyDrop` panics of mechanism 1. Fixed by
//!    binding a `common::FirefoxGuard` over the replacement's `results.pid`
//!    before any assertion at both sites.
//!
//! [`live_151_root_cause_documented`] reproduces mechanism 1 live: it drives
//! both the pre-fix (`ManuallyDrop`) and fixed (normal binding) shapes
//! against a real Firefox process inside a `catch_unwind`'d panic and
//! asserts on actual PID liveness afterward — proof, not a hypothesis.
//!
//! ## Theme C — whole-suite guarantee, testable in chunks
//!
//! [`live_151_chunk_a_leaves_no_orphans`] / [`live_151_chunk_b_leaves_no_orphans`]
//! nest a full chunk run (the exact `live_1` / `--skip live_1` split this
//! plan's "Environment quirks" section documents) and assert zero
//! ff-rdp-spawned Firefox processes — identified via Theme A's marker, not a
//! raw `pgrep` count (see the plan's Notes on why: a raw count is noisy in a
//! shared sandbox and doesn't distinguish ff-rdp's processes from the
//! developer's own browser) — survive it. Expensive by design (nests
//! ~6 minutes of live tests); gated behind an extra opt-in env var so a
//! normal live-suite run doesn't silently double in duration — see
//! [`SUITE_CHECK_ENV`].
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli --test live live_151 -- --nocapture

use std::panic::AssertUnwindSafe;
use std::process::Command;
use std::time::Duration;

use crate::common::{LiveFirefox, ff_rdp_bin, kill_pid, live_tests_enabled, pid_alive};

/// Poll until `pid_alive(pid)` is `false` or `timeout` elapses. Mirrors
/// `live_146_suite_reliability.rs`'s identical helper — `kill_pid`'s
/// `SIGKILL` is asynchronous, so a liveness probe taken immediately after a
/// kill can still observe "alive" for a few milliseconds.
fn wait_until_dead(pid: u32, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if !pid_alive(pid) {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

// ---------------------------------------------------------------------------
// Theme A
// ---------------------------------------------------------------------------

/// AC: `live_151_leaked_profile_names_its_test` — a profile directory
/// spawned via `LiveFirefox` carries an `.ff-rdp-owner-test` marker naming
/// this exact test, readable straight off disk with no other context — the
/// artifact-alone traceability iter-151 Theme A adds on top of the
/// pre-existing owner-PID marker.
#[test]
#[ignore = "requires Firefox — FF_RDP_LIVE_TESTS=1"]
fn live_151_leaked_profile_names_its_test() {
    if !live_tests_enabled() {
        return;
    }

    let (ff, envelope) = LiveFirefox::headless_on_random_port_with_args(&[]);

    let profile_path = envelope["results"]["profile_path"]
        .as_str()
        .expect("live_151_leaked_profile_names_its_test: results.profile_path")
        .to_owned();

    let marker_path = std::path::Path::new(&profile_path).join(".ff-rdp-owner-test");
    let marker_contents = std::fs::read_to_string(&marker_path).unwrap_or_else(|e| {
        panic!(
            "live_151_leaked_profile_names_its_test: FAIL — {} missing or unreadable: {e} \
             (the Theme A marker was not written)",
            marker_path.display()
        )
    });
    let marker_contents = marker_contents.trim();

    let this_test = std::thread::current()
        .name()
        .expect("cargo test always names the test thread")
        .to_owned();
    assert!(
        this_test.ends_with("live_151_leaked_profile_names_its_test"),
        "live_151_leaked_profile_names_its_test: sanity check failed — unexpected test \
         thread name {this_test:?}"
    );
    assert_eq!(
        marker_contents,
        this_test,
        "live_151_leaked_profile_names_its_test: FAIL — owner-test marker at {} names \
         {marker_contents:?}, expected the spawning test {this_test:?}",
        marker_path.display()
    );

    eprintln!(
        "live_151_leaked_profile_names_its_test: PASS — {} names its spawning test \
         ({this_test}) from the artifact alone",
        marker_path.display()
    );

    // `ff` drops here — normal cleanup, no deliberate leak needed to prove
    // the marker was written correctly.
    drop(ff);
}

// ---------------------------------------------------------------------------
// Theme B — mechanism, proven live
// ---------------------------------------------------------------------------

/// AC: `live_151_root_cause_documented` — reproduces, live, the exact
/// mechanism this iteration's Theme B fixed: a `LiveFirefox` guard
/// suppressed via `ManuallyDrop` (the pre-fix pattern removed from
/// `live_90_daemon_lifecycle.rs`, `live_daemon_stop_mdn.rs`, and
/// `live_142_daemon_stop_pid_honesty.rs` by this iteration) leaves Firefox
/// alive when a panic strikes before cleanup runs; the fixed pattern (the
/// guard stays a normal binding) does not.
///
/// This drives both shapes against a real Firefox process and asserts on
/// actual PID liveness afterward — the same "prove it on the wire" bar
/// every `pre_fix_repro_*` test in this suite holds itself to, not a
/// restated hypothesis.
#[test]
#[ignore = "requires Firefox — FF_RDP_LIVE_TESTS=1"]
fn live_151_root_cause_documented() {
    if !live_tests_enabled() {
        return;
    }

    // --- Pre-fix shape: guard suppressed via ManuallyDrop before a panic ---
    let ff = LiveFirefox::headless_on_random_port();
    let leaked_pid = ff.pid();
    let outcome = std::panic::catch_unwind(AssertUnwindSafe(|| {
        // The exact pre-fix idiom: suppress Drop, then panic before any
        // cleanup code runs — modeling a failed `daemon stop` assertion.
        let _keep = std::mem::ManuallyDrop::new(ff);
        panic!("live_151 probe: simulated assertion failure before cleanup (pre-fix shape)");
    }));
    assert!(outcome.is_err(), "probe closure must panic");

    // Give the (nonexistent) cleanup a moment it will never get.
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        pid_alive(leaked_pid),
        "live_151_root_cause_documented: pid {leaked_pid} is already dead — expected it to \
         still be alive, proving the ManuallyDrop pattern leaks Firefox when a panic strikes \
         before cleanup runs (the pre-151 bug this iteration fixes)"
    );
    eprintln!(
        "live_151_root_cause_documented: confirmed — ManuallyDrop pattern leaked pid \
         {leaked_pid}; cleaning it up manually now"
    );
    kill_pid(leaked_pid);
    assert!(
        wait_until_dead(leaked_pid, Duration::from_secs(2)),
        "live_151_root_cause_documented: manual cleanup of the deliberately-leaked pid \
         {leaked_pid} failed — the test harness itself is broken"
    );

    // --- Fixed shape: guard stays a normal binding across the same panic ---
    let ff2 = LiveFirefox::headless_on_random_port();
    let protected_pid = ff2.pid();
    let outcome2 = std::panic::catch_unwind(AssertUnwindSafe(|| {
        // `ff2` is moved into the closure as a normal binding — the fixed
        // pattern. It drops (killing Firefox) as part of the panic's
        // unwind, exactly like every other live test in this suite.
        let _ff2 = ff2;
        panic!("live_151 probe: simulated assertion failure before cleanup (fixed shape)");
    }));
    assert!(outcome2.is_err(), "probe closure must panic");
    assert!(
        wait_until_dead(protected_pid, Duration::from_secs(2)),
        "live_151_root_cause_documented: FAIL — pid {protected_pid} survived a panic even \
         with the guard kept as a normal binding (the iter-151 fix regressed)"
    );

    eprintln!(
        "live_151_root_cause_documented: PASS — mechanism confirmed live: ManuallyDrop \
         leaked pid {leaked_pid}, the fixed pattern reaped pid {protected_pid} through the \
         same panic"
    );
}

// ---------------------------------------------------------------------------
// Theme C — whole-suite guarantee, testable in chunks
// ---------------------------------------------------------------------------

/// Env var gating [`live_151_chunk_a_leaves_no_orphans`] and
/// [`live_151_chunk_b_leaves_no_orphans`]. Both nest an entire live-test
/// chunk (~6 minutes) inside themselves via `std::env::current_exe()`, so
/// they must NOT run just because `FF_RDP_LIVE_TESTS=1` and
/// `--include-ignored` are set — that would silently double the runtime of
/// every live-suite invocation, including CI's weekly `live.yml` run (which
/// runs the whole suite with no filter). Opt in explicitly:
///
///   FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_SUITE_CHECK=1 cargo test-live -p ff-rdp-cli \
///       --test live live_151_chunk_a -- --nocapture
const SUITE_CHECK_ENV: &str = "FF_RDP_LIVE_SUITE_CHECK";

fn suite_check_enabled() -> bool {
    std::env::var(SUITE_CHECK_ENV).as_deref() == Ok("1")
}

/// Resolve the real profile root via `ff-rdp profiles list` — the same
/// discovery `live_175_failed_launch_profile.rs`'s `profile_root()` helper
/// uses, rather than duplicating `secure_profile_root()`'s resolution logic
/// (unreachable from an integration-test binary — see that module's doc
/// comment). `live_96_profile_cleanup.rs` used to be the canonical example
/// here too, before its `$FF_RDP_HOME`-isolated
/// `live_profiles_prune_removes_all_when_no_firefox_running` was deleted as
/// a duplicate of `tests/e2e/profiles.rs` (iter-188 PR review) — see that
/// module's current doc comment.
fn profile_root() -> Option<String> {
    let out = Command::new(ff_rdp_bin())
        .args(["profiles", "list"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    json["results"]["path"].as_str().map(str::to_owned)
}

/// Scan `root` for `ff-rdp-profile-*` directories whose owner-PID marker
/// names a still-alive process, returning `(dir, pid, spawning_test)`
/// triples. Duplicates `live_96_profile_cleanup.rs`'s helper of the same
/// name (not shared — see that file's own duplication note: no `[lib]`
/// target for an integration-test binary to import from).
fn live_owned_profile_dirs(root: &str) -> Vec<(std::path::PathBuf, u32, Option<String>)> {
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
            let pid: u32 = std::fs::read_to_string(e.path().join(".ff-rdp-owner-pid"))
                .ok()?
                .trim()
                .parse()
                .ok()?;
            if !pid_alive(pid) {
                return None;
            }
            let test_name = std::fs::read_to_string(e.path().join(".ff-rdp-owner-test"))
                .ok()
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty());
            Some((e.path(), pid, test_name))
        })
        .collect()
}

/// Runs the named chunk (`filter` positional args, `skip` exclusion args) as
/// a nested subprocess of this very test binary, then asserts zero
/// newly-appeared ff-rdp-spawned Firefox processes survive it.
///
/// Design notes:
/// - Uses `std::env::current_exe()` rather than shelling out to `cargo
///   test` — the binary is already built; re-invoking `cargo test` would
///   pay to re-resolve the workspace and risks a rebuild race with whatever
///   invoked *this* test.
/// - Always passes `--skip live_151` AND clears [`SUITE_CHECK_ENV`] for the
///   child: chunk A's filter (`"live_1"`) is a substring of every
///   `live_151_*` test's own name (`live_151_...` contains `live_1`), so
///   without the skip a nested chunk-A run would re-select — and re-nest —
///   these very tests, recursing without bound. Both guards are redundant
///   with each other by design, the project's own "belt and suspenders"
///   idiom (see `live_96_profile_cleanup.rs`).
/// - Does NOT assert the nested run's own exit status — a chunk may contain
///   failures unrelated to this check (e.g. the known-red `live_109`, see
///   this plan's "Environment quirks" notes); this check's only concern is
///   whether Firefox processes survive the run.
/// - Any live-owned profile already present before the nested run started
///   is excluded from the survivor count (best-effort — an unrelated
///   concurrent leak in a shared sandbox shouldn't fail *this* check, but is
///   still logged loudly since it means the result may be contaminated).
fn assert_chunk_leaves_no_orphans(caller: &str, filter: &[&str], skip: &[&str]) {
    if !live_tests_enabled() {
        eprintln!("{caller}: FF_RDP_LIVE_TESTS not set — skipping");
        return;
    }
    if !suite_check_enabled() {
        eprintln!(
            "{caller}: opt-in gate {SUITE_CHECK_ENV}=1 not set — skipping (this test nests an \
             entire live-suite chunk and is not part of a normal live-suite run)"
        );
        return;
    }
    let Some(root) = profile_root() else {
        eprintln!("{caller}: could not resolve the profile root via `profiles list` — skipping");
        return;
    };

    let pre_existing = live_owned_profile_dirs(&root);
    if !pre_existing.is_empty() {
        eprintln!(
            "{caller}: {} live-owned profile dir(s) already present before this run started \
             — {pre_existing:?} — the environment isn't clean; this check's result may be \
             contaminated by an unrelated leak",
            pre_existing.len()
        );
    }

    let mut args: Vec<String> = vec![
        "--include-ignored".to_owned(),
        "--test-threads=1".to_owned(),
        "--skip".to_owned(),
        "live_151".to_owned(),
    ];
    for s in skip {
        args.push("--skip".to_owned());
        args.push((*s).to_owned());
    }
    args.extend(filter.iter().map(|s| (*s).to_owned()));

    let exe = std::env::current_exe().expect("current_exe");
    eprintln!(
        "{caller}: spawning nested chunk run: {} {args:?}",
        exe.display()
    );
    let start = std::time::Instant::now();
    let status = Command::new(&exe)
        .args(&args)
        .env_remove(SUITE_CHECK_ENV)
        .status();
    let elapsed = start.elapsed();
    match &status {
        Ok(s) => eprintln!(
            "{caller}: nested chunk run finished in {:.1}s, exit status {s}",
            elapsed.as_secs_f64()
        ),
        Err(e) => eprintln!("{caller}: nested chunk run failed to spawn: {e}"),
    }

    // A harness that cannot re-exec itself is a broken harness, not a skip.
    // Without this the survivor assertion below passes trivially — zero tests
    // ran, so nothing could have leaked — and a ~6 minute leak check reports
    // green having verified nothing at all.
    let status = status.unwrap_or_else(|e| {
        panic!(
            "{caller}: FAIL — could not spawn the nested chunk run ({e}); this opt-in check \
                must not report success without actually executing the chunk"
        )
    });
    // A non-zero exit is tolerated (an unrelated test in the chunk may be red,
    // which is not this check's business) but termination by signal is not:
    // a killed chunk did not finish, so "no survivors" would prove nothing.
    assert!(
        status.code().is_some(),
        "{caller}: FAIL — the nested chunk run was terminated by signal rather than exiting \
         ({status}); its survivor count is not meaningful"
    );

    let survivors: Vec<_> = live_owned_profile_dirs(&root)
        .into_iter()
        .filter(|(dir, pid, _)| !pre_existing.iter().any(|(d, p, _)| d == dir && p == pid))
        .collect();

    assert!(
        survivors.is_empty(),
        "{caller}: FAIL — {} ff-rdp-spawned Firefox process(es) survived the nested chunk run: \
         {}",
        survivors.len(),
        survivors
            .iter()
            .map(|(dir, pid, test_name)| format!(
                "{} (pid {pid}, spawned by {})",
                dir.display(),
                test_name.as_deref().unwrap_or("unknown test")
            ))
            .collect::<Vec<_>>()
            .join(", ")
    );

    eprintln!(
        "{caller}: PASS — zero surviving ff-rdp-spawned Firefox processes after the nested \
         chunk run ({:.1}s)",
        elapsed.as_secs_f64()
    );
}

/// AC: `live_151_chunk_a_leaves_no_orphans` — a full `live_1`-filtered
/// chunk run (the exact filter this plan's "Environment quirks" section
/// documents) leaves zero surviving ff-rdp-spawned Firefox processes.
///
/// Expensive — nests an entire chunk run (~6 minutes) inside itself; opt in
/// with `FF_RDP_LIVE_SUITE_CHECK=1` (see [`assert_chunk_leaves_no_orphans`]).
#[test]
#[ignore = "requires Firefox — FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_SUITE_CHECK=1"]
fn live_151_chunk_a_leaves_no_orphans() {
    assert_chunk_leaves_no_orphans("live_151_chunk_a_leaves_no_orphans", &["live_1"], &[]);
}

/// AC: `live_151_chunk_b_leaves_no_orphans` — the complementary
/// `--skip live_1` chunk leaves zero surviving ff-rdp-spawned Firefox
/// processes, and (per the plan) `live_96_profile_cleanup`'s precondition
/// passes without manual cleanup — a non-empty survivor list here is
/// exactly what would make that precondition fail.
///
/// Expensive — see [`live_151_chunk_a_leaves_no_orphans`].
#[test]
#[ignore = "requires Firefox — FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_SUITE_CHECK=1"]
fn live_151_chunk_b_leaves_no_orphans() {
    assert_chunk_leaves_no_orphans("live_151_chunk_b_leaves_no_orphans", &[], &["live_1"]);
}
