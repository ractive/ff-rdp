//! iter-171 — a leaked profile dir's owner-PID marker must not resurrect its
//! ownership once the OS recycles that PID.
//!
//! `.ff-rdp-owner-pid` outlives the Firefox it names: `LiveFirefox::drop` (and
//! any `kill`) ends the process but leaves the directory, marker and all. Every
//! ownership check in the codebase then asks `kill(pid, 0)`, which cannot tell
//! "the Firefox that wrote this marker" from "whatever process holds that PID
//! now". Once the PID is recycled the dead profile reads as **live-owned**, so
//! the age-gated `profiles prune` skips it permanently and
//! `live_96_profile_cleanup`'s precondition fires against a browser that has
//! been gone for half an hour.
//!
//! The unit half (`util::profile_dir::tests::pre_fix_repro_*`) pins the
//! grading logic against forged markers. This is the end-to-end half: a really
//! launched Firefox, its real marker pair, its real death, and the real
//! `ff-rdp profiles prune` selection afterwards.
//!
//! Both assertions below fail on `main`: the start marker does not exist there
//! at all, and the age-gated prune keeps the recycled-PID directory.
//!
//! PID recycling is *simulated* (the dead profile's PID marker is overwritten
//! with this live test process's PID) rather than waited for. Measured on the
//! iteration's dev machine, PIDs are handed out at ~229/second under saturated
//! spawning against a `PID_MAX` of 99 999 — a real wrap is reachable inside one
//! live sweep but costs minutes of pure `fork` to force, and the on-disk state
//! it produces is byte-for-byte what this test writes directly.

use std::process::Command;

use crate::common::{
    FirefoxGuard, ff_rdp_bin, ff_rdp_launch_command, kill_pid_and_wait, live_tests_enabled,
};

/// Owner-PID marker written inside every ff-rdp-managed profile dir; mirrors
/// the product's private `util::profile_dir::OWNER_PID_MARKER`, duplicated for
/// the same reason `live_96_profile_cleanup.rs`, `live_151_residual_leak.rs`
/// and `live_168_drop_waits_for_exit.rs` duplicate it — this crate ships no
/// `[lib]` target for an integration-test binary to import from.
const OWNER_PID_MARKER: &str = ".ff-rdp-owner-pid";

/// Owner-start (process identity) marker, iter-171. Same duplication
/// rationale as [`OWNER_PID_MARKER`]; mirrors
/// `util::profile_dir::OWNER_START_MARKER`.
const OWNER_START_MARKER: &str = ".ff-rdp-owner-start";

/// AC: `live_171_recycled_owner_pid_no_longer_reads_as_live`
///
/// 1. `ff-rdp launch --headless` a real Firefox into a managed profile.
/// 2. Assert the profile carries **both** owner markers — the PID and the
///    process-identity token. (Fails on `main`: no token is ever written.)
/// 3. Kill the Firefox and wait for it to actually go away, so the directory
///    is a genuinely leaked one.
/// 4. Simulate PID reuse by pointing the surviving PID marker at this live
///    test process, leaving the dead Firefox's identity token in place.
/// 5. Assert an age-gated `profiles prune` still selects the directory.
///    (Fails on `main`: the live PID makes it look owned, so the age-gated
///    path excludes it outright and it is never reclaimed at any age.)
///
/// Step 5 runs with `--dry-run` deliberately: a real age-gated prune against
/// the shared per-user profile root would reclaim unrelated directories, and
/// the property under test is the *selection*, not the `remove_dir_all`. The
/// directory is removed by this test itself at the end either way.
#[test]
#[ignore = "requires Firefox and FF_RDP_LIVE_TESTS=1"]
fn live_171_recycled_owner_pid_no_longer_reads_as_live() {
    if !live_tests_enabled() {
        return;
    }

    let port = 7171;
    let launch = ff_rdp_launch_command()
        .args(["launch", "--headless", "--debug-port", &port.to_string()])
        .output()
        .expect("live_171: could not spawn `ff-rdp launch`");
    assert!(
        launch.status.success(),
        "live_171: `ff-rdp launch` failed — stdout={} stderr={}",
        String::from_utf8_lossy(&launch.stdout),
        String::from_utf8_lossy(&launch.stderr)
    );

    let json: serde_json::Value =
        serde_json::from_slice(&launch.stdout).expect("live_171: launch stdout is not JSON");
    let results = &json["results"];
    let pid = results["pid"]
        .as_u64()
        .and_then(|p| u32::try_from(p).ok())
        .expect("live_171: launch JSON must expose a numeric results.pid");
    // Own the process from the moment we know its PID. Step 3 kills it deliberately, but any
    // panic before that would otherwise leak a live Firefox holding a managed profile dir —
    // which is precisely the failure mode this iteration exists to close. Killing an
    // already-dead PID in the guard's drop is a no-op, so the deliberate kill stays authoritative.
    let _guard = FirefoxGuard::new(pid);
    let profile_dir = std::path::PathBuf::from(
        results["profile_path"]
            .as_str()
            .expect("live_171: launch JSON must expose results.profile_path"),
    );

    // 2. Both markers must exist, and the identity token must be non-empty.
    let pid_marker = profile_dir.join(OWNER_PID_MARKER);
    let start_marker = profile_dir.join(OWNER_START_MARKER);
    let recorded_pid = std::fs::read_to_string(&pid_marker)
        .unwrap_or_else(|e| panic!("live_171: {} unreadable: {e}", pid_marker.display()));
    assert_eq!(
        recorded_pid.trim().parse::<u32>().ok(),
        Some(pid),
        "live_171: the owner-PID marker must name the Firefox launch reported"
    );
    let recorded_token = std::fs::read_to_string(&start_marker).unwrap_or_else(|e| {
        panic!(
            "live_171: {} is missing — a managed profile must record its owner's \
             process identity, not just its PID (this is what fails on main): {e}",
            start_marker.display()
        )
    });
    assert!(
        !recorded_token.trim().is_empty(),
        "live_171: the recorded process-identity token must not be blank"
    );

    // 3. Kill the owner and confirm the leak: process gone, directory (and its
    //    now-stale markers) still on disk. This is the state every interrupted
    //    or daemon-stop-less live test leaves behind.
    kill_pid_and_wait(pid);
    assert!(
        profile_dir.is_dir(),
        "live_171: the profile dir is expected to survive its Firefox — if it \
         no longer does, this test's premise is gone and it should be rewritten, \
         not relaxed"
    );

    // 4. Simulate PID reuse: the marker now names a live process that is not
    //    the Firefox which wrote it.
    let recycled = std::process::id();
    std::fs::write(&pid_marker, format!("{recycled}\n"))
        .expect("live_171: could not forge the recycled PID marker");
    assert!(
        std::fs::read_to_string(&start_marker).is_ok(),
        "live_171: the identity token must survive the PID marker rewrite"
    );

    // 5. The age-gated prune must still select it.
    let prune = Command::new(ff_rdp_bin())
        .args(["profiles", "prune", "--older-than", "0s", "--dry-run"])
        .output()
        .expect("live_171: could not spawn `ff-rdp profiles prune`");
    let prune_stdout = String::from_utf8_lossy(&prune.stdout).into_owned();
    assert!(
        prune.status.success(),
        "live_171: `profiles prune --dry-run` failed — stdout={prune_stdout} stderr={}",
        String::from_utf8_lossy(&prune.stderr)
    );
    let prune_json: serde_json::Value =
        serde_json::from_str(&prune_stdout).expect("live_171: prune stdout is not JSON");
    let basename = profile_dir
        .file_name()
        .and_then(|n| n.to_str())
        .expect("live_171: profile dir has a UTF-8 basename")
        .to_owned();
    let selected = prune_json["results"]["would_remove"]
        .as_array()
        .expect("live_171: prune JSON must expose results.would_remove")
        .iter()
        .any(|v| v.as_str().is_some_and(|s| s.contains(&basename)));

    // Clean up before asserting, so a failure does not also leak the dir it is
    // complaining about.
    let _ = std::fs::remove_dir_all(&profile_dir);

    assert!(
        selected,
        "live_171: {basename}'s owner PID was recycled, so the profile is \
         abandoned and an age-gated prune must reclaim it. On main the live \
         PID makes it read as owned and it is skipped at every age — \
         would_remove={}",
        prune_json["results"]["would_remove"]
    );

    eprintln!("live_171: PASS — recycled owner PID no longer reads as a live owner");
}
