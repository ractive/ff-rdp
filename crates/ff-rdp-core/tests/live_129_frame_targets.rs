//! Live test for iteration 129 Theme A: `enumerate_frame_targets` against a
//! real cross-origin (data: top + `https://example.com` child) fixture.
//!
//! Requires a live Firefox instance (gated by `FF_RDP_LIVE_TESTS=1`) and
//! network access to `https://example.com`. Firefox must already be running
//! with a debugger server (see `.claude/CLAUDE.md` "Test Fixtures" recording
//! workflow) — this test does not launch Firefox itself.
//!
//! Run with:
//! ```sh
//! firefox -no-remote -profile /tmp/ff-rdp-test-profile \
//!   --start-debugger-server 6000 --headless &
//! FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-core --test live_129_frame_targets -- --include-ignored
//! ```

mod support;

use std::time::Duration;

use ff_rdp_core::{
    RdpConnection, RootActor, TabActor, WindowGlobalTarget, enumerate_frame_targets,
};
use support::recording::{firefox_port, should_run_live};

const TIMEOUT: Duration = Duration::from_secs(15);

/// AC: `live_129_frame_targets_enumerated` — on a fixture embedding a
/// cross-origin iframe (data: top + `https://example.com` child),
/// `enumerate_frame_targets` yields >=2 targets including a non-top target
/// with the example.com url and a distinct `processID` from the top target
/// where Fission actually spawns it out-of-process (verified live against
/// Firefox 152/153 in `kb/research/frame-targets.md`; asserted here as a
/// non-fatal diagnostic rather than a hard requirement, since OOP scheduling
/// is a Firefox implementation detail this test does not control).
#[test]
#[ignore = "requires live Firefox + network — FF_RDP_LIVE_TESTS=1"]
fn live_129_frame_targets_enumerated() {
    if !should_run_live() {
        eprintln!("live_129_frame_targets_enumerated: set FF_RDP_LIVE_TESTS=1 to run");
        return;
    }

    let port = firefox_port();
    let mut conn = RdpConnection::connect("127.0.0.1", port, TIMEOUT)
        .expect("connect to Firefox — is it running with --start-debugger-server?");

    let tabs = RootActor::list_tabs(conn.transport_mut()).expect("list_tabs");
    let tab = tabs
        .iter()
        .find(|t| t.selected)
        .or_else(|| tabs.first())
        .expect("need at least one tab");
    let tab_actor = tab.actor.clone();

    let target = TabActor::get_target(conn.transport_mut(), &tab_actor).expect("getTarget");

    let fixture_url = r#"data:text/html,<h1>top</h1><iframe src="https://example.com"></iframe>"#;
    WindowGlobalTarget::navigate_to(conn.transport_mut(), &target.actor, fixture_url)
        .expect("navigateTo fixture");

    // No doc-complete wait plumbing at this (core) layer — give the
    // cross-origin child frame time to load and form() server-side.
    std::thread::sleep(Duration::from_secs(2));

    let watcher_actor =
        TabActor::get_watcher_with_options(conn.transport_mut(), &tab_actor, Some(true))
            .expect("getWatcher(isServerTargetSwitchingEnabled: true)");

    let targets = enumerate_frame_targets(
        conn.transport_mut(),
        &watcher_actor,
        Duration::from_millis(1500),
    )
    .expect("enumerate_frame_targets");

    eprintln!(
        "live_129_frame_targets_enumerated: {} targets: {:?}",
        targets.len(),
        targets
            .iter()
            .map(|t| (t.url.clone(), t.is_top_level, t.process_id))
            .collect::<Vec<_>>()
    );

    assert!(
        targets.len() >= 2,
        "expected >=2 targets (top + example.com child), got {}: {targets:?}",
        targets.len()
    );

    let top = targets
        .iter()
        .find(|t| t.is_top_level)
        .expect("a top-level target must be present");
    let child = targets
        .iter()
        .find(|t| !t.is_top_level && t.url.as_deref().is_some_and(|u| u.contains("example.com")))
        .expect("a non-top target with the example.com url must be present");

    assert!(
        child.console_actor.is_some(),
        "the example.com child target must carry its own consoleActor: {child:?}"
    );

    match (top.process_id, child.process_id) {
        (Some(top_pid), Some(child_pid)) if top_pid != child_pid => {
            eprintln!(
                "live_129_frame_targets_enumerated: confirmed out-of-process child (top pid={top_pid}, child pid={child_pid})"
            );
        }
        (Some(top_pid), Some(child_pid)) => {
            eprintln!(
                "live_129_frame_targets_enumerated: child ran in-process this run (pid={top_pid} == {child_pid}) — non-fatal, Fission scheduling is environment-dependent"
            );
        }
        _ => {
            eprintln!(
                "live_129_frame_targets_enumerated: processID missing on one side — non-fatal"
            );
        }
    }

    eprintln!("live_129_frame_targets_enumerated: PASSED");
}
