//! iter-168 — dropping a [`LiveFirefox`] must leave **no** ff-rdp-managed
//! profile dir whose owner-PID marker still reads as alive.
//!
//! This is the live half of the iteration: `iter_168_harness_kill_wait.rs`
//! pins the waiting contract against stub probes, and this asserts the same
//! contract end-to-end against a really-launched Firefox, through the exact
//! scan (`live_owned_profile_dirs`) whose precondition failure in iter-165's
//! sweep started all this.
//!
//! Deliberately asserts **immediately** after the drop returns, with no sleep
//! and no retry loop. A sleep here would test nothing: the pre-168 code passes
//! that version of the test. "The drop returned, therefore the process is
//! gone" is the property, so the assertion must sit exactly where the drop
//! returns.

use std::process::Command;

use crate::common::{LiveFirefox, ff_rdp_bin, live_tests_enabled, pid_alive};

/// Owner-PID marker written inside every ff-rdp-managed profile dir; mirrors
/// the product's private `util::profile_dir::OWNER_PID_MARKER`, duplicated for
/// the same reason `live_96_profile_cleanup.rs` and `live_151_residual_leak.rs`
/// duplicate it — this crate ships no `[lib]` target to import from.
const OWNER_PID_MARKER: &str = ".ff-rdp-owner-pid";

/// Owner-test marker (iter-151 Theme A), same duplication rationale.
const OWNER_TEST_MARKER: &str = ".ff-rdp-owner-test";

/// Scan `root` for `ff-rdp-profile-*` dirs whose owner-PID marker names a
/// still-alive process, as `(dir, pid, spawning_test)`.
///
/// Byte-for-byte the scan `live_96_profile_cleanup`'s precondition runs, so a
/// pass here means that precondition cannot fire on this test's account.
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

/// The managed profile root, as the product itself reports it.
fn profile_root() -> String {
    let out = Command::new(ff_rdp_bin())
        .args(["profiles", "list"])
        .output()
        .expect("live_168_adjacent_tests_leave_no_live_owner: profiles list spawn failed");
    assert!(
        out.status.success(),
        "live_168_adjacent_tests_leave_no_live_owner: profiles list must succeed — stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).expect(
        "live_168_adjacent_tests_leave_no_live_owner: profiles list stdout is not valid JSON",
    );
    json["results"]["path"]
        .as_str()
        .expect(
            "live_168_adjacent_tests_leave_no_live_owner: profiles list JSON must expose \
             results.path",
        )
        .to_owned()
}

/// AC `live_168_adjacent_tests_leave_no_live_owner`: after a `LiveFirefox` is
/// dropped, `live_owned_profile_dirs` reports no entry owned by that pid.
///
/// Reproduces, in one test, the two-test interaction that failed in iter-165's
/// sweep: `live_128_meta_route` dropped its guard and `live_96_profile_cleanup`
/// then found the dropped instance's pid still alive under the profile root.
#[test]
#[ignore = "requires Firefox and FF_RDP_LIVE_TESTS=1"]
fn live_168_adjacent_tests_leave_no_live_owner() {
    if !live_tests_enabled() {
        return;
    }

    let root = profile_root();
    let ff = LiveFirefox::headless_on_random_port();
    let pid = ff.pid();

    // Detector check: unless the marker mechanism actually sees this instance
    // while it is alive, the post-drop assertion below would pass vacuously.
    let before = live_owned_profile_dirs(&root);
    assert!(
        before.iter().any(|(_, p, _)| *p == pid),
        "live_168_adjacent_tests_leave_no_live_owner: a running LiveFirefox (pid {pid}) must \
         appear as a live owner under {root}; without that this test cannot detect a leak. \
         Saw: {before:?}"
    );

    drop(ff);

    // No sleep, no retry — see the module doc.
    assert!(
        !pid_alive(pid),
        "live_168_adjacent_tests_leave_no_live_owner: pid {pid} still reads as alive the \
         instant LiveFirefox::drop returned. Drop signals SIGKILL and must then wait for the \
         process to actually leave the process table (see common::kill_pid_and_wait)."
    );
    let after = live_owned_profile_dirs(&root);
    assert!(
        !after.iter().any(|(_, p, _)| *p == pid),
        "live_168_adjacent_tests_leave_no_live_owner: profile dir under {root} is still owned \
         by the dropped instance's pid {pid} — this is exactly what makes \
         `profiles prune --all` refuse in live_96_profile_cleanup. Saw: {after:?}"
    );
}
