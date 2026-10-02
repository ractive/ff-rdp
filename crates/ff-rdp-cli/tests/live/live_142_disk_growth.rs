//! Live tests for iter-142 Theme B: disk growth (temp profiles).
//!
//! Dogfooding session 63 observed 62 temp profiles / 2.7 GB accumulate in a
//! single day, because `prune_orphan_profiles`'s default 7-day age gate never
//! let a dead-owner profile go before a week had passed. iter-142 reclaims a
//! dead-owner profile immediately regardless of age.
//!
//! Run with:
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live live_142_disk_growth -- --nocapture

use std::time::Duration;

use crate::common::{FirefoxGuard, ff_rdp_launch_command, kill_pid, live_tests_enabled};

/// Attempt to bind `:0` to discover a free port.
fn free_port() -> Option<u16> {
    let l = std::net::TcpListener::bind("127.0.0.1:0").ok()?;
    Some(l.local_addr().ok()?.port())
}

// The RAII guard used throughout this file is `common::FirefoxGuard`.
//
// This file cannot reuse `common::LiveFirefox` outright: it needs the raw
// `launch` envelope (`results.profile_path`), which `LiveFirefox::try_launch`
// doesn't expose. The test below used to launch via a bare `Command` with no guard at all and
// rely entirely on a *manual*, later `kill_pid` call for cleanup — the
// exact "no RAII guard across an assertion" shape iter-146 fixed in
// `live_96_profile_cleanup.rs`'s `launch_headless` (see that file's doc
// comment) but left unfixed here: any assertion between spawn and the
// manual `kill_pid` panicking left Firefox alive with nothing left to reap
// it — a real, still-open instance of the exact bug class iter-146 was
// meant to close suite-wide.

/// Launch Firefox headless via the CLI on a freshly discovered port and
/// return `(guard, port, results)` — `results` is the `results` object of
/// the launch envelope. The guard is constructed immediately once a PID is
/// confirmed, so every assertion downstream is guard-protected — see
/// [`FirefoxGuard`]'s doc comment.
/// Panics on any launch failure (iter-158 Theme D) — see
/// `common::LiveFirefox::headless_on_random_port`.
fn launch_headless() -> (FirefoxGuard, u16, serde_json::Value) {
    let port = free_port().expect("bind 127.0.0.1:0 to discover a free port");
    let out = ff_rdp_launch_command()
        .args(["launch", "--headless", "--debug-port", &port.to_string()])
        .output()
        .expect("spawn `ff-rdp launch`");
    // iter-242 Theme B: the guard comes first, before the success assertion
    // and before any parsing. The previous order — assert, parse envelope,
    // parse `results.pid`, *then* construct the guard — left three panicking
    // `expect`s between a Firefox that is running and the thing that reaps it,
    // which is the identical window this file's own doc comment says it
    // closed.
    let guard = crate::common::guard_launched_firefox(&out);
    assert!(
        out.status.success(),
        "launch_headless: `ff-rdp launch --headless --debug-port {port}` exited {}\n  stdout: {}\n  stderr: {}",
        out.status,
        String::from_utf8_lossy(&out.stdout).trim(),
        String::from_utf8_lossy(&out.stderr).trim(),
    );
    let guard = guard.expect("launch_headless: successful launch reported no results.pid");
    let json: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("launch stdout is JSON");
    let results = json
        .get("results")
        .expect("launch envelope has a `results` object")
        .clone();
    (guard, port, results)
}

/// AC: `live_142_profile_growth_bounded`
///
/// Policy under test: a temp profile whose owner PID is confirmed dead is
/// reclaimed by the very next `launch`'s opportunistic sweep — immediately,
/// not after any age threshold (`prune_orphan_profiles`, iter-142 Theme B).
///
/// 1. Launch Firefox headless (creates a managed temp profile + owner-PID
///    marker).
/// 2. Force-kill it directly (SIGKILL), so the profile dir is only
///    reclaimable via the orphan sweep.
/// 3. Launch a second Firefox headless immediately (same process, no delay,
///    no artificial aging of the first profile's mtime).
/// 4. Assert instance A's profile directory is gone — proves growth is
///    bounded by "next launch", not by waiting out an age threshold.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_142_profile_growth_bounded() {
    if !live_tests_enabled() {
        return;
    }

    let (guard_a, _port_a, results_a) = launch_headless();
    let pid_a = guard_a.pid();
    let profile_a = results_a["profile_path"]
        .as_str()
        .expect("results.profile_path")
        .to_owned();

    assert!(
        std::path::Path::new(&profile_a).exists(),
        "live_142_profile_growth_bounded: profile {profile_a} must exist right after launch"
    );

    // Force-kill instance A directly, so the profile dir is orphaned exactly
    // like a crash or `kill -9` would leave it.
    kill_pid(pid_a);
    std::thread::sleep(Duration::from_millis(500));

    eprintln!(
        "live_142_profile_growth_bounded: instance A (pid {pid_a}) force-killed, \
         profile {profile_a} now orphaned"
    );

    // Immediately launch a second instance — no artificial delay. If growth
    // were still bounded only by the old 7-day age gate, instance A's
    // fresh (seconds-old) profile would still be sitting there.
    let (guard_b, port_b, _results_b) = launch_headless();
    let pid_b = guard_b.pid();

    assert!(
        !std::path::Path::new(&profile_a).exists(),
        "live_142_profile_growth_bounded: FAIL — orphaned profile {profile_a} \
         must be reclaimed by the very next launch's opportunistic sweep, \
         not left to accumulate for days"
    );

    eprintln!(
        "live_142_profile_growth_bounded: PASS — orphaned profile {profile_a} \
         reclaimed by the next launch (instance B on port {port_b}, pid {pid_b})"
    );

    // Clean up instance B.
    kill_pid(pid_b);
}
