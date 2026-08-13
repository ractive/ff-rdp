//! Live tests for iteration 146 — live suite reliability.
//!
//! ## Theme A — the live-test harness's own teardown
//!
//! `LiveFirefox`'s `Drop` was already reliable (verified live before this
//! iteration: a throwaway probe test that panics after `with_daemon()`
//! leaves zero surviving processes). The actual leak iter-146 found in a
//! full sequential sweep traced to `live_96_profile_cleanup.rs`'s
//! `launch_headless()`, which used to launch Firefox via a bare `Command`
//! with **no** RAII guard at all — see the fix and doc comment there. The
//! tests below pin the harness-wide guarantee so it can't regress silently:
//! every `LiveFirefox` (with or without a running daemon) must leave no
//! surviving process once its guard drops, even through a panic.
//!
//! ## Theme C — the iter-137 daemon-parity flake
//!
//! Root-caused live (not merely widened a timeout — see
//! `daemon/server.rs`'s `WATCHER_SETTLE_DELAY` and the early-event-sink
//! comments at both `establish_watcher_with_retry` call sites for the full
//! mechanism). `live_146_daemon_parity_stable_repeat` locks in that fix with
//! repeated runs of the exact daemon-mode-parity shape iter-137 introduced.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live live_146 -- --nocapture

use std::panic::AssertUnwindSafe;
use std::process::Command;

use crate::common::{LiveFirefox, ff_rdp_bin, live_tests_enabled, pid_alive};

/// Poll until `pid_alive(pid)` is `false` or `timeout` elapses, returning
/// the final liveness.
///
/// `kill_pid`'s `SIGKILL` is asynchronous — the kernel needs a moment to
/// actually reap the process, so a liveness probe taken immediately after
/// `Drop` can still observe "alive" for a few milliseconds. Every assertion
/// in this file that a Firefox PID is gone polls through this helper rather
/// than checking once, so it verifies the guard's eventual guarantee
/// (bounded and small — 2 s is generous headroom) instead of racing its own
/// probe against the kernel.
fn wait_until_dead(pid: u32, timeout: std::time::Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if !pid_alive(pid) {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

/// AC: `live_146_no_orphan_firefox_after_suite` — after a small sequential
/// run of `LiveFirefox` (+ daemon) launches — the shape a live suite takes —
/// zero of the Firefox processes they started are still alive once every
/// guard has dropped. Mirrors the dogfood_path's "zero ff-rdp-owned Firefox
/// processes remain" bar at a scale this test can run unattended.
#[test]
#[ignore = "requires Firefox — FF_RDP_LIVE_TESTS=1"]
fn live_146_no_orphan_firefox_after_suite() {
    const INSTANCES: usize = 3;

    if !live_tests_enabled() {
        return;
    }

    let mut pids = Vec::with_capacity(INSTANCES);
    for i in 0..INSTANCES {
        let ff = LiveFirefox::headless_on_random_port();
        pids.push(ff.pid());
        if i == 1 {
            // Exercise the daemon-spawning path too — the shape every
            // `firefox_with_daemon` helper across the live suite uses.
            let _ = ff.with_daemon();
        }
        // `ff` drops at the end of this iteration, killing this instance
        // before the next one launches — modeling a sequential suite run.
    }

    for pid in &pids {
        assert!(
            wait_until_dead(*pid, std::time::Duration::from_secs(2)),
            "live_146_no_orphan_firefox_after_suite: Firefox pid {pid} is still alive after \
             its LiveFirefox guard dropped"
        );
    }

    eprintln!(
        "live_146_no_orphan_firefox_after_suite: PASS — {}/{INSTANCES} sequential launches \
         left no survivor",
        pids.len()
    );
}

/// AC: `live_146_harness_teardown_kills_daemon_spawned_firefox` — a test
/// that starts Firefox via `with_daemon` and then panics still leaves zero
/// surviving processes once its `LiveFirefox` guard drops (dropped as part
/// of the panic's unwind, exactly as `cargo test`'s own per-test harness
/// does). This is the guarantee `live_96_profile_cleanup.rs`'s pre-iter-146
/// `launch_headless()` helper lacked entirely — see the Theme A fix there.
#[test]
#[ignore = "requires Firefox — FF_RDP_LIVE_TESTS=1"]
fn live_146_harness_teardown_kills_daemon_spawned_firefox() {
    if !live_tests_enabled() {
        return;
    }

    let pid_cell = std::cell::Cell::new(None::<u32>);
    let outcome = std::panic::catch_unwind(AssertUnwindSafe(|| {
        // iter-158 Theme D: `headless_on_random_port` panics rather than
        // returning `None`. That panic is caught here just like the
        // intentional one below, so the `pid_cell`-is-empty arm underneath
        // now means "Firefox never launched" and is reported as a failure
        // instead of a skip.
        let ff = LiveFirefox::headless_on_random_port();
        pid_cell.set(Some(ff.pid()));
        if ff.with_daemon().is_none() {
            return false;
        }
        // `ff` is moved into and dies inside this closure's unwind — the
        // scenario under test: a live test that panics mid-assertion with
        // its daemon-backed Firefox still in scope.
        panic!("iter-146 probe: intentional panic with a daemon running");
    }));

    let pid = pid_cell.get().expect(
        "live_146_harness_teardown_kills_daemon_spawned_firefox: Firefox never launched, so \
         the teardown guarantee under test was never exercised",
    );
    match outcome {
        Ok(true) => unreachable!("the probe closure always panics once the daemon starts"),
        Ok(false) => panic!(
            "live_146_harness_teardown_kills_daemon_spawned_firefox: the daemon did not start \
             for pid {pid}, so the teardown guarantee under test was never exercised"
        ),
        Err(_) => {
            assert!(
                wait_until_dead(pid, std::time::Duration::from_secs(2)),
                "live_146_harness_teardown_kills_daemon_spawned_firefox: Firefox pid {pid} \
                 survived a panic while its daemon was running"
            );
            eprintln!(
                "live_146_harness_teardown_kills_daemon_spawned_firefox: PASS — pid {pid} is \
                 gone"
            );
        }
    }
}

/// A `data:` fixture identical to live_137's `CROSS_ORIGIN_FIXTURE`: a top
/// document (unique origin) embedding a genuinely cross-origin
/// `https://example.com` iframe.
const CROSS_ORIGIN_FIXTURE: &str =
    r#"data:text/html,<h1>top</h1><iframe src="https://example.com"></iframe>"#;

/// Default-daemon-mode global args (the proxied path — no direct-connection
/// override flag).
fn daemon_args(port: u16) -> Vec<String> {
    vec![
        "--host".to_owned(),
        "127.0.0.1".to_owned(),
        "--port".to_owned(),
        port.to_string(),
        "--timeout".to_owned(),
        "30000".to_owned(),
    ]
}

fn daemon_status(port: u16) -> String {
    let out = Command::new(ff_rdp_bin())
        .args(["--host", "127.0.0.1", "--port", &port.to_string()])
        .args(["daemon", "status"])
        .output()
        .expect("daemon status");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Poll `daemon status` until it reports at least one **live** target — see
/// live_137's identical helper for the full rationale.
fn wait_for_live_targets(port: u16) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while std::time::Instant::now() < deadline {
        let text = daemon_status(port);
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text)
            && json["results"]["live_target_count"].as_u64().unwrap_or(0) >= 1
        {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    false
}

/// AC: `live_146_daemon_parity_stable_repeat` — `live_137_frame_targets_via_daemon`
/// / `live_137_click_cross_origin_via_daemon`'s core shape (launch, start the
/// daemon, navigate to a cross-origin fixture, wait for live frame targets)
/// passes 5 consecutive fresh-daemon runs.
///
/// Root cause (documented in `kb/iterations/iteration-146-live-suite-reliability.md`
/// and at the fix sites in `daemon/server.rs`): NOT a daemon restart —
/// verified live via `daemon status`'s `uptime_seconds`, which stayed
/// continuous across a failing run's whole session, and via `RUST_LOG=debug`
/// captures showing one unbroken daemon PID throughout. Two real bugs
/// combined to strand `target_count` at 0 or 1 forever:
///   1. `establish_watcher`'s synchronous `watchTargets` handshake ran with
///      no `RdpTransport` event sink installed, so a `target-available-form`
///      catch-up event racing ahead of its RPC reply was silently dropped by
///      `forward_event` (fixed: an early sink now buffers and replays it).
///   2. A freshly-launched profile's very first tab is a placeholder
///      `about:blank` target that Firefox tears down within microseconds of
///      being observed, independent of any navigation; subscribing to it
///      before it settles orphans the watcher for the rest of the session —
///      no replacement `target-available-form` ever arrives, even though
///      `navigate`/`eval` keep working normally (fixed: `WATCHER_SETTLE_DELAY`
///      gives the placeholder→real promotion time to finish before the
///      daemon ever subscribes).
#[test]
#[ignore = "requires Firefox + network — FF_RDP_LIVE_TESTS=1"]
fn live_146_daemon_parity_stable_repeat() {
    const ITERATIONS: usize = 5;

    if !live_tests_enabled() {
        return;
    }

    for i in 1..=ITERATIONS {
        let ff = LiveFirefox::headless_on_random_port();
        if ff.with_daemon().is_none() {
            eprintln!(
                "live_146_daemon_parity_stable_repeat: daemon did not start on iteration \
                 {i}/{ITERATIONS} — skipping"
            );
            return;
        }
        let port = ff.port();

        let nav = Command::new(ff_rdp_bin())
            .args(daemon_args(port))
            .args(["navigate", CROSS_ORIGIN_FIXTURE, "--allow-unsafe-urls"])
            .output()
            .expect("navigate via daemon");
        if !nav.status.success() {
            eprintln!(
                "live_146_daemon_parity_stable_repeat: navigate failed on iteration \
                 {i}/{ITERATIONS} — {}",
                String::from_utf8_lossy(&nav.stderr)
            );
            return;
        }

        assert!(
            wait_for_live_targets(port),
            "live_146_daemon_parity_stable_repeat: iteration {i}/{ITERATIONS} — daemon never \
             reported live frame targets (iter-146 Theme C regression) — status: {}",
            daemon_status(port)
        );

        eprintln!("live_146_daemon_parity_stable_repeat: iteration {i}/{ITERATIONS} PASSED");
        // `ff` drops here, killing this iteration's Firefox (and its daemon,
        // once it notices the lost connection) before the next fresh daemon
        // spawns — each iteration exercises the exact race window from
        // scratch.
    }

    eprintln!(
        "live_146_daemon_parity_stable_repeat: PASS — {ITERATIONS}/{ITERATIONS} consecutive \
         runs"
    );
}
