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

// iter-242 Theme E: the owner-marker scan and the two marker-name literals
// used to be copy-pasted here (and in `live_151_residual_leak.rs`, and in the
// since-deleted `live_96` precondition). The justification recorded in
// iteration 151 — "no `[lib]` target for an integration-test binary to import
// from" — was wrong: every one of those files is a *module of this same
// `tests/live` binary*, and `tests/common/mod.rs` exists for exactly this. The
// duplication that genuinely cannot be removed is the one against the product
// crate's private constants, which `common` now carries in one place.
use crate::common::live_owned_profile_dirs;

/// The managed profile root, as the product itself reports it.
fn profile_root() -> String {
    let out = Command::new(ff_rdp_bin())
        .args(["profiles", "list"])
        .output()
        .expect("live_168_adjacent_tests_leave_no_live_owner: profiles list spawn failed");
    assert!(
        out.status.success(),
        "live_168_adjacent_tests_leave_no_live_owner: profiles list must succeed — {}",
        crate::common::output_note(&out)
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
