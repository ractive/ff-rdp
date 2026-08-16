//! iter-168 — `LiveFirefox::drop` must *wait* for the killed Firefox to leave
//! the process table, not merely signal it.
//!
//! iter-165's dual-gate `live-sweep` produced:
//!
//! ```text
//! ---- live_96_profile_cleanup::live_profiles_prune_removes_all_when_no_firefox_running stdout ----
//!   precondition violated — 1 ff-rdp-managed profile dir(s) … are still owned
//!   by a live process … (pid 59653, spawned by live_128_meta_route)
//! ```
//!
//! `live_128_meta_route`'s guard *had* dropped. `kill_pid` sends `SIGKILL` and
//! returns in ~20 µs; the kernel then takes ~20 ms to finish tearing the
//! process down, and the test process is not Firefox's parent so it never reaps
//! it either. Throughout that window `kill(pid, 0)` — the probe behind
//! `pid_alive`, and behind `live_96`'s owner-PID precondition — still reports
//! the process as alive. iteration-168 Theme A measured that window directly:
//! **16–27 ms across ten headless-Firefox kills**, at load averages 6.5 to 54.
//! It is real, it is unconditional, and this iteration closes it.
//!
//! It is **not**, however, what produced the failure quoted above, and this
//! file does not claim it is. Under `--test-threads=1` the two tests are 176
//! tests apart in the live binary's execution order (`live_128_meta_route` is
//! #25 of 260, `live_96`'s prune test is #201) — minutes, not milliseconds. Ten
//! adjacent-pair runs on a pristine `main` at load averages 106→268 reproduced
//! the precondition failure **0 times**. See
//! `kb/iterations/iteration-168-livefirefox-drop-does-not-wait-for-exit.md`
//! Theme A for the measurements and for the stale-marker/PID-reuse hypothesis
//! that carries the remaining explanation.
//!
//! These tests pin the waiting contract against stub liveness probes, so the
//! prompt-exit, already-dead and timeout paths are all verifiable with no
//! Firefox and no unkillable process anywhere in sight. (Firefox-free and
//! ungated, so they run on every `cargo test`.)

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

#[path = "common/mod.rs"]
mod common;

use common::{
    DEFAULT_KILL_WAIT_MS, KILL_WAIT_TIMEOUT_ENV, kill_wait_timeout_from, wait_for_pid_exit,
    wait_for_pid_exit_with,
};

/// AC `unit_168_drop_waits_for_pid_exit`: the wait helper returns promptly for
/// a process that exits, returns immediately for an already-dead pid, and hits
/// its bounded timeout for a pid that never dies — all without launching
/// Firefox.
#[test]
fn unit_168_drop_waits_for_pid_exit() {
    // (1) A process that exits shortly after the signal: the helper must
    // actually wait for it (not report "still alive" the way the pre-168 drop
    // effectively did by never asking), and must return as soon as it goes,
    // not burn the whole budget.
    let probes = AtomicUsize::new(0);
    let signalled_at = Instant::now();
    let dies_after = Duration::from_millis(30);
    let started = Instant::now();
    let waited = wait_for_pid_exit_with(Duration::from_secs(5), || {
        probes.fetch_add(1, Ordering::SeqCst);
        signalled_at.elapsed() < dies_after
    });
    let elapsed = started.elapsed();
    let waited = waited.expect("a process that exits within the budget must be observed exiting");
    assert!(
        waited >= dies_after,
        "the helper must wait until the process is actually gone, not return early: {waited:?}"
    );
    assert!(
        elapsed < Duration::from_secs(1),
        "the helper must return as soon as the pid disappears, not burn the 5 s budget: {elapsed:?}"
    );
    assert!(
        probes.load(Ordering::SeqCst) >= 2,
        "the helper must re-probe rather than check once and give up (probes={})",
        probes.load(Ordering::SeqCst)
    );

    // (2) An already-dead pid: one probe, no sleep. `Drop` runs on every live
    // test's teardown, so a helper that unconditionally slept would tax ~150
    // tests for a wait none of them needs.
    let probes = AtomicUsize::new(0);
    let started = Instant::now();
    let waited = wait_for_pid_exit_with(Duration::from_secs(5), || {
        probes.fetch_add(1, Ordering::SeqCst);
        false
    });
    assert!(
        waited.is_some(),
        "an already-dead pid must report as exited"
    );
    assert_eq!(
        probes.load(Ordering::SeqCst),
        1,
        "an already-dead pid must cost exactly one probe"
    );
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "an already-dead pid must not block: {:?}",
        started.elapsed()
    );

    // (3) A pid that never dies: bounded give-up, not a hang. A live suite that
    // wedges here would be strictly worse than the flake this fixes.
    let started = Instant::now();
    let waited = wait_for_pid_exit_with(Duration::from_millis(200), || true);
    assert_eq!(
        waited, None,
        "a pid that never exits must report None at the deadline"
    );
    assert!(
        started.elapsed() >= Duration::from_millis(200),
        "the helper must honour its full budget before giving up: {:?}",
        started.elapsed()
    );
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "the bound must actually bound; took {:?}",
        started.elapsed()
    );
}

/// A zero budget still probes once — "no time left" must not degrade into
/// "reported alive without ever asking", which is precisely the pre-168
/// behaviour.
#[test]
fn unit_168_zero_budget_still_probes_once() {
    let probes = AtomicUsize::new(0);
    let waited = wait_for_pid_exit_with(Duration::ZERO, || {
        probes.fetch_add(1, Ordering::SeqCst);
        false
    });
    assert!(waited.is_some());
    assert_eq!(probes.load(Ordering::SeqCst), 1);

    let probes = AtomicUsize::new(0);
    let waited = wait_for_pid_exit_with(Duration::ZERO, || {
        probes.fetch_add(1, Ordering::SeqCst);
        true
    });
    assert_eq!(waited, None);
    assert_eq!(probes.load(Ordering::SeqCst), 1);
}

/// The real `pid_alive`-backed entry point, exercised against two pids whose
/// liveness is not in doubt: this very process (alive → must hit the bound) and
/// a child that has already been spawned, exited **and reaped** (dead → must
/// return at once).
#[test]
fn unit_168_wait_for_pid_exit_reads_real_pids() {
    let started = Instant::now();
    assert_eq!(
        wait_for_pid_exit(std::process::id(), Duration::from_millis(150)),
        None,
        "the current process is alive, so the wait must time out rather than \
         claim it exited"
    );
    assert!(started.elapsed() >= Duration::from_millis(150));

    // A reaped child is genuinely gone from the process table. Spawned and
    // waited here rather than hard-coding a pid, because an invented pid could
    // collide with a live process.
    let mut child = std::process::Command::new(if cfg!(windows) { "cmd" } else { "sh" })
        .args(if cfg!(windows) {
            ["/C", "exit 0"]
        } else {
            ["-c", "exit 0"]
        })
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn a trivially short-lived child");
    let pid = child.id();
    child.wait().expect("reap the short-lived child");

    let started = Instant::now();
    let waited = wait_for_pid_exit(pid, Duration::from_secs(5));
    assert!(
        waited.is_some(),
        "a reaped child's pid must read as exited, not time out"
    );
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "a dead pid must not cost a wait: {:?}",
        started.elapsed()
    );
}

/// The default budget must dwarf the window Theme A measured (16–27 ms over ten
/// headless-Firefox kills at load averages 6.5–54), so an ordinary teardown
/// never reaches the give-up path.
#[test]
fn unit_168_default_budget_dwarfs_the_measured_window() {
    let measured_worst_case = Duration::from_millis(27);
    assert!(
        kill_wait_timeout_from(None) >= measured_worst_case * 100,
        "default budget {:?} must leave two orders of magnitude of headroom \
         over the measured {measured_worst_case:?} window",
        kill_wait_timeout_from(None)
    );
}

/// `FF_RDP_TEST_KILL_WAIT_TIMEOUT_MS` raises (or lowers) the budget, and junk
/// falls back to the default instead of to "don't wait" — a zero or misspelled
/// override that disabled waiting would silently reinstate the pre-168 race.
#[test]
fn unit_168_kill_wait_timeout_env_override() {
    assert_eq!(
        kill_wait_timeout_from(Some("250")),
        Duration::from_millis(250)
    );
    assert_eq!(
        kill_wait_timeout_from(Some("  30000 ")),
        Duration::from_millis(30_000),
        "surrounding whitespace must not defeat the override"
    );
    for junk in ["", "0", "abc", "-1", "2.5"] {
        assert_eq!(
            kill_wait_timeout_from(Some(junk)),
            Duration::from_millis(DEFAULT_KILL_WAIT_MS),
            "{junk:?} must fall back to the default, never to a zero wait"
        );
    }
    // The name the diagnostic tells the reader to set is the name that is read.
    assert_eq!(KILL_WAIT_TIMEOUT_ENV, "FF_RDP_TEST_KILL_WAIT_TIMEOUT_MS");
}
