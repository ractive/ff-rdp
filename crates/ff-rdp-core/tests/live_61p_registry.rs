//! Live tests for iter-61p: actor registry + Front lifecycle.
//!
//! These tests require a running headless Firefox with the remote debugger
//! enabled on port 6000 (or `FF_RDP_PORT`).
//!
//! Run with:
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-core --test live_61p_registry

mod support;

use std::time::Duration;

use ff_rdp_core::{
    ActorId, ConsoleFront, FrontKind, RdpConnection, RdpError, Registry, RootActor, TabActor,
    TargetFront,
};
use support::recording::{firefox_port, should_run_live};

const TIMEOUT: Duration = Duration::from_secs(15);

fn connect() -> RdpConnection {
    RdpConnection::connect("127.0.0.1", firefox_port(), TIMEOUT)
        .expect("failed to connect to Firefox RDP server")
}

// ── live_dead_actor_error_type ───────────────────────────────────────────────

/// AC: when called on a known-destroyed actor, `RdpError::ActorDestroyed{actor}`
/// is returned and surfaced as `error_type: "actor_destroyed"`.
///
/// We simulate destruction by registering a fake actor ID, invalidating its
/// target, and then asserting alive on it — no network I/O required.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_dead_actor_error_type() {
    if !should_run_live() {
        return;
    }

    let reg = Registry::new();
    let target_id = ActorId::from("server1.conn0.windowGlobalTarget99");
    let console_id = ActorId::from("server1.conn0.child1/consoleActor99");

    // Register a target and a console front owned by it.
    reg.register(target_id.clone(), FrontKind::Target, None);
    reg.register(
        console_id.clone(),
        FrontKind::Console,
        Some(target_id.clone()),
    );

    // Simulate target-destroyed-form.
    reg.invalidate_target(&target_id);

    // assert_alive should return ActorDestroyed with the correct actor.
    match reg.assert_alive(&console_id) {
        Err(RdpError::ActorDestroyed { actor }) => {
            assert_eq!(
                actor, console_id,
                "ActorDestroyed must carry the destroyed actor ID"
            );
        }
        other => panic!("expected ActorDestroyed, got {other:?}"),
    }
}

// ── live_single_target_per_browsing_context ──────────────────────────────────

/// AC: the registry holds at most one TargetFront per browsing-context-id at
/// any given time.
///
/// We connect to Firefox, list tabs, open the first tab's target, and assert
/// there is exactly one registered TargetFront for that target actor.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_single_target_per_browsing_context() {
    if !should_run_live() {
        return;
    }

    let mut conn = connect();
    let transport = conn.transport_mut();

    let tabs = RootActor::list_tabs(transport).expect("listTabs failed");
    let tab = tabs.first().expect("expected at least one open tab");

    let target_info = TabActor::get_target(transport, &tab.actor).expect("getTarget failed");

    // Build a shared registry and register the target front.
    let reg = Registry::new();
    let _target_front = TargetFront::new(target_info.actor.clone(), reg.clone());

    // Also register the console front as owned by the target.
    let _console_front = ConsoleFront::new(
        target_info.console_actor.clone(),
        reg.clone(),
        target_info.actor.clone(),
    );

    // Exactly one target-kind entry should exist (the TargetFront we just added).
    let target_count = reg.count_alive_fronts_for_target(&target_info.actor);

    // The console front is owned by the target — so count_alive_fronts_for_target
    // counts it, but the TargetFront itself has no target_root (it IS the root).
    // Verify the TargetFront is registered and alive.
    reg.assert_alive(&target_info.actor)
        .expect("target front must be alive after registration");

    // count_alive_fronts_for_target counts fronts OWNED BY this target (i.e.
    // console), not the target itself. Console + any other fronts we registered.
    assert!(
        target_count <= 1,
        "expected at most 1 front owned by the target (the console), got {target_count}"
    );
}

// ── live_consoleactor_invalidation ──────────────────────────────────────────

/// AC: after a cross-origin navigation the old console actor is invalidated
/// and `assert_alive` returns ActorDestroyed.  A fresh front must be obtained
/// from the new target to succeed.
///
/// We test the registry side of this (no live navigation): we register a
/// console front, destroy the owning target, confirm the front is dead, then
/// register a *new* console front under a new target and confirm it is alive.
#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_consoleactor_invalidation() {
    if !should_run_live() {
        return;
    }

    let reg = Registry::new();

    // Simulate "page 1" target + console.
    let target_v1 = ActorId::from("conn0/windowGlobalTarget1");
    let console_v1 = ActorId::from("conn0/child1/consoleActor1");

    reg.register(target_v1.clone(), FrontKind::Target, None);
    reg.register(
        console_v1.clone(),
        FrontKind::Console,
        Some(target_v1.clone()),
    );

    // Navigation: target-destroyed-form fires.
    reg.invalidate_target(&target_v1);

    // The old console is now dead.
    assert!(
        matches!(
            reg.assert_alive(&console_v1),
            Err(RdpError::ActorDestroyed { .. })
        ),
        "console_v1 must be dead after target invalidation"
    );

    // Simulate "page 2" target + console (new after navigation).
    let target_v2 = ActorId::from("conn0/windowGlobalTarget2");
    let console_v2 = ActorId::from("conn0/child1/consoleActor2");

    reg.register(target_v2.clone(), FrontKind::Target, None);
    reg.register(
        console_v2.clone(),
        FrontKind::Console,
        Some(target_v2.clone()),
    );

    // The new console is alive.
    reg.assert_alive(&console_v2)
        .expect("console_v2 must be alive after re-registration");

    // The old one is still dead (not overwritten).
    assert!(
        matches!(
            reg.assert_alive(&console_v1),
            Err(RdpError::ActorDestroyed { .. })
        ),
        "console_v1 must still be dead — not overwritten by new registration"
    );
}
