/// Live test for Theme I (iter-84): a second command after `navigate`
/// connects to the correct tab and does not hit a stale ActorId cached
/// from a previous session.
///
/// The race is: navigate closes+reopens the tab (changes ActorId), then
/// a second command (e.g. `dom stats`) uses the cached ActorId from the
/// pre-navigate state, getting a "No such actor" error from Firefox.
///
/// AC: live_stale_tab_race — dom stats succeeds immediately after navigate
///     on a fresh Firefox session (no "No such actor" error)
use crate::common::{live_network_tests_enabled, live_tests_enabled};
use std::process::Command;

fn ff_rdp_bin() -> String {
    env!("CARGO_BIN_EXE_ff-rdp").to_string()
}

/// Theme I: running `navigate` then immediately `dom stats` does not produce
/// a "No such actor" error due to a stale tab handle.
///
/// Pre-condition: Firefox running with `--start-debugger-server 6000`.
/// Post-condition: `dom stats` exits 0 after `navigate` completes.
#[test]
#[ignore = "requires FF_RDP_LIVE_TESTS=1 and running Firefox"]
fn live_stale_tab_race_no_such_actor_after_navigate() {
    if !live_tests_enabled() || !live_network_tests_enabled() {
        return;
    }

    // First navigate — establishes a tab actor.
    let nav1 = Command::new(ff_rdp_bin())
        .args(["navigate", "https://example.com/"])
        .output()
        .expect("navigate 1 failed");
    assert!(
        nav1.status.success(),
        "navigate 1 failed: {}",
        String::from_utf8_lossy(&nav1.stderr)
    );

    // Second navigate — may change the tab actor ID.
    let nav2 = Command::new(ff_rdp_bin())
        .args(["navigate", "https://example.org/"])
        .output()
        .expect("navigate 2 failed");
    assert!(
        nav2.status.success(),
        "navigate 2 failed: {}",
        String::from_utf8_lossy(&nav2.stderr)
    );

    // dom stats after re-navigate — must not use a stale actor ID.
    let stats = Command::new(ff_rdp_bin())
        .args(["dom", "stats"])
        .output()
        .expect("dom stats failed");

    let stderr = String::from_utf8_lossy(&stats.stderr);
    assert!(
        stats.status.success(),
        "Theme I regression: dom stats failed after double-navigate: {stderr}"
    );
    assert!(
        !stderr.contains("No such actor"),
        "Theme I regression: 'No such actor' in stderr — stale tab cache bug: {stderr}"
    );
}
