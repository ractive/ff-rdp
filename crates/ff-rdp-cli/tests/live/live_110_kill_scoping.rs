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

use std::process::Command;
use std::time::Duration;

use crate::common::{RawFirefox, ff_rdp_bin, live_tests_enabled, pid_alive};

/// AC: `live_110_replace_never_kills_foreign_firefox` — a Firefox launched
/// outside ff-rdp (no owner-PID marker) survives an `ff-rdp launch --replace`
/// targeting its port, and ff-rdp reports a refusal rather than terminating it.
#[test]
#[ignore = "requires a live Firefox instance (FF_RDP_LIVE_TESTS=1)"]
fn live_110_replace_never_kills_foreign_firefox() {
    if !live_tests_enabled() {
        eprintln!("live_110_replace_never_kills_foreign_firefox: skipped (FF_RDP_LIVE_TESTS != 1)");
        return;
    }

    let Some(raw) = RawFirefox::headless_on_random_port() else {
        eprintln!(
            "live_110_replace_never_kills_foreign_firefox: skipped — could not launch a raw Firefox"
        );
        return;
    };
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
    let output = Command::new(ff_rdp_bin())
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

    // raw drops here → kills the foreign Firefox we spawned and cleans its profile.
    drop(raw);
}
