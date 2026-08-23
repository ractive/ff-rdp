//! iter-110 Theme A0 — kill-scoping: an `ff-rdp` operation must NEVER signal a
//! Firefox process that ff-rdp did not itself launch.
//!
//! Background (2026-07-09 incident): the `stop_prior_instance` port-owner
//! fallback in `daemon::client` used to SIGKILL whatever process was listening
//! on the requested RDP port. When a user's own interactive Firefox happened to
//! be on ff-rdp's default port 6000, `ff-rdp launch --replace` (and the live
//! harness) repeatedly killed it. The fix gates that fallback on the iter-97
//! owner-PID marker: only a process ff-rdp spawned (which planted a marker in a
//! managed profile under our per-user root) may be signalled.
//!
//! This test launches Firefox **directly** (no `ff-rdp launch`, so no marker),
//! points `ff-rdp launch --replace` at its port, and asserts the browser is
//! still alive afterward — a foreign process is never killed.
//!
//! iter-191 added a second phase to the same test. The guard above covers the
//! port-owner fallback; the branch *above* that one — "a launch record matches
//! this port and its PID is alive" — never reached it, and on 2026-08-23 took
//! a week-old leaked record as ownership proof and signalled the PID the OS
//! had since reissued. Phase B plants such a record deliberately so both
//! branches are exercised on every sweep, not just whichever one a random port
//! happens to land in.

use std::time::Duration;

use crate::common::{RawFirefox, ff_rdp_launch_command, live_tests_enabled, pid_alive};

/// AC: `live_110_replace_never_kills_foreign_firefox` — a Firefox launched
/// outside ff-rdp (no owner-PID marker) survives an `ff-rdp launch --replace`
/// targeting its port, and ff-rdp reports a refusal rather than terminating it.
///
/// Phase A reaches that guarantee through the port-owner fallback; phase B
/// (iter-191) reaches it through the launch-record branch, with a stale record
/// planted for the target port.
#[test]
#[ignore = "requires a live Firefox instance (FF_RDP_LIVE_TESTS=1)"]
fn live_110_replace_never_kills_foreign_firefox() {
    if !live_tests_enabled() {
        eprintln!("live_110_replace_never_kills_foreign_firefox: skipped (FF_RDP_LIVE_TESTS != 1)");
        return;
    }

    let raw = RawFirefox::headless_on_random_port();
    let foreign_pid = raw.pid();
    let port = raw.port();

    // Sanity: the foreign browser is alive before we provoke ff-rdp.
    assert!(
        pid_alive(foreign_pid),
        "precondition: the raw Firefox (pid {foreign_pid}) must be alive before --replace"
    );

    // Provoke the port-owner kill path: --replace on the exact port the foreign
    // Firefox holds. With no daemon record / registry / owner marker for this
    // PID, ff-rdp reaches the step-3 fallback — which must now REFUSE.
    let output = ff_rdp_launch_command()
        .args([
            "launch",
            "--replace",
            "--headless",
            "--debug-port",
            &port.to_string(),
        ])
        .output()
        .expect("run ff-rdp launch --replace");

    // Give any (buggy) kill signal time to land before we assert survival.
    std::thread::sleep(Duration::from_millis(500));

    // THE core assertion: the foreign Firefox must still be alive. This is the
    // guarantee the 2026-07-09 incident violated.
    assert!(
        pid_alive(foreign_pid),
        "REGRESSION: ff-rdp launch --replace killed a foreign Firefox (pid {foreign_pid}) it \
         did not launch — the kill-scoping guard failed"
    );

    // ff-rdp should refuse (non-zero exit) and explain it will not stop a
    // process it does not own. The refusal text is emitted on the error path.
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}\n{stderr}");
    assert!(
        !output.status.success(),
        "ff-rdp launch --replace should fail (not succeed by killing a foreign browser); \
         stdout={stdout} stderr={stderr}"
    );
    assert!(
        combined.contains("did not launch") || combined.contains("does not own"),
        "refusal message must explain ff-rdp will not stop an unowned process; got: {combined}"
    );

    // -----------------------------------------------------------------
    // Phase B (iter-191): the same refusal, reached through the *launch
    // record* branch instead of the port-owner fallback.
    //
    // The 2026-08-23 live sweep failed this test's message assertion because
    // the random port it picked collided with a seven-day-old
    // `launch-record.<port>.json` left on the machine. `stop_prior_instance`
    // matched on `rec.port == port && is_alive(rec.pid)` alone, took that
    // stale file as ownership proof, and signalled the process group of a PID
    // the OS had since reissued to an unrelated desktop application — then
    // reported "port still in use after stopping the prior instance".
    //
    // Reproduce it deterministically rather than waiting for another
    // collision: plant a record for this port naming the foreign Firefox
    // itself, with a start token that cannot match (exactly what a recycled
    // PID's recorded token looks like). If the ownership gate regresses, the
    // record hands ff-rdp a kill permit for `foreign_pid` and the survival
    // assertion below is what catches it.
    //
    // `FF_RDP_HOME` scopes the planted record to a temp dir, so a sweep run
    // never leaves a fabricated record in the operator's real `~/.ff-rdp`.
    let home = tempfile::tempdir().expect("tempdir for planted launch record");
    let record_dir = home.path().join(".ff-rdp");
    std::fs::create_dir_all(&record_dir).expect("create planted .ff-rdp");
    std::fs::write(
        record_dir.join(format!("launch-record.{port}.json")),
        format!(
            r#"{{"pid": {foreign_pid}, "port": {port}, "headless": true,
                 "launched_at": "2026-08-16T20:32:13.855708Z",
                 "profile_dir": "/tmp/ff-rdp-does-not-exist",
                 "start_token": "0.000000"}}"#
        ),
    )
    .expect("plant stale launch record");

    let planted = ff_rdp_launch_command()
        .env("FF_RDP_HOME", home.path())
        .args([
            "launch",
            "--replace",
            "--headless",
            "--debug-port",
            &port.to_string(),
        ])
        .output()
        .expect("run ff-rdp launch --replace against a planted stale record");

    std::thread::sleep(Duration::from_millis(500));

    assert!(
        pid_alive(foreign_pid),
        "REGRESSION (iter-191): a stale launch record naming pid {foreign_pid} was treated as \
         ownership proof and ff-rdp signalled it"
    );

    let planted_out = format!(
        "{}\n{}",
        String::from_utf8_lossy(&planted.stdout),
        String::from_utf8_lossy(&planted.stderr)
    );
    assert!(
        !planted.status.success(),
        "launch --replace must fail rather than claim it freed the port; got: {planted_out}"
    );
    assert!(
        planted_out.contains("did not launch") || planted_out.contains("does not own"),
        "the launch-record branch must refuse in the same words as the port-owner branch; \
         got: {planted_out}"
    );
    assert!(
        !planted_out.contains("still in use after stopping the prior instance"),
        "that message claims ff-rdp stopped something it owned; it stopped nothing. \
         got: {planted_out}"
    );

    // raw drops here → kills the foreign Firefox we spawned and cleans its profile.
    drop(raw);
}
