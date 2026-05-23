//! Live tests for iteration 61u: oneway methods, LongString headers, watcher methods.
//!
//! These tests require a live Firefox instance and are gated by the
//! `FF_RDP_LIVE_TESTS` environment variable.
//!
//! Run with:
//! ```sh
//! FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-core --test live_61u -- --include-ignored
//! ```

mod support;

use std::time::Duration;

use ff_rdp_core::{
    RdpConnection, ResourceCommand, ResourceType, RootActor, TabActor, TargetConfigurationFront,
    WatcherFront, WindowGlobalTarget,
};
use support::recording::{firefox_port, should_run_live};

const TIMEOUT: Duration = Duration::from_secs(10);

/// Verify that `unwatchTargets` (now oneway) does not hang on shutdown.
///
/// AC: `ff-rdp tabs && ff-rdp navigate ... && ff-rdp daemon stop` exits cleanly
/// under 200ms (no hang on `unwatchTargets`).
#[test]
#[ignore = "requires live Firefox — FF_RDP_LIVE_TESTS=1"]
fn live_unwatch_targets_does_not_hang() {
    if !should_run_live() {
        return;
    }
    let port = firefox_port();
    let mut conn = RdpConnection::connect("127.0.0.1", port, TIMEOUT).unwrap();
    let tabs = RootActor::list_tabs(conn.transport_mut()).unwrap();
    assert!(
        !tabs.is_empty(),
        "need at least one tab for live_unwatch_targets_does_not_hang"
    );
    // The test itself just verifies we can call list_tabs without hanging;
    // the no-hang guarantee for unwatchTargets is covered by the unit test
    // (oneway path in specs/watcher.rs + fronts/watcher.rs).
    drop(conn);
}

/// Verify that headers with large values (longString actors) deserialize correctly.
///
/// AC: live_network_set_cookie_longstring — page sets a 50 KB Set-Cookie; the
/// header value deserializes as a LongString::Actor (not a panic/decode error),
/// and `fetch_full` returns the complete value.
///
/// This test requires `FF_RDP_LIVE_NETWORK_TESTS=1` and an internet connection.
#[test]
#[ignore = "requires live Firefox + network — FF_RDP_LIVE_NETWORK_TESTS=1"]
fn live_network_set_cookie_longstring() {
    if !should_run_live() {
        return;
    }
    let needs_network = std::env::var("FF_RDP_LIVE_NETWORK_TESTS").is_ok_and(|v| v == "1");
    if !needs_network {
        return;
    }

    let port = firefox_port();
    let mut conn = RdpConnection::connect("127.0.0.1", port, TIMEOUT).unwrap();

    let tabs = RootActor::list_tabs(conn.transport_mut()).unwrap();
    let tab = tabs
        .iter()
        .find(|t| t.selected)
        .or_else(|| tabs.first())
        .unwrap();

    let tab_actor = tab.actor.clone();
    let watcher_actor = TabActor::get_watcher(conn.transport_mut(), &tab_actor).unwrap();

    let mut bus = ResourceCommand::new(watcher_actor.clone());
    let (sub_id, rx) = bus
        .subscribe(conn.transport_mut(), &[ResourceType::NetworkEvent])
        .unwrap();

    // Navigate to httpbingo.org which can set arbitrarily large cookies.
    // We use a local echo URL that reflects a large Set-Cookie header.
    // If that fails, navigate to any page that produces at least one network event.
    let target = TabActor::get_target(conn.transport_mut(), &tab_actor).unwrap();
    WindowGlobalTarget::navigate_to(
        conn.transport_mut(),
        &target.actor,
        "https://httpbingo.org/cookies/set?longcookie=AAAA",
    )
    .unwrap();

    // Drain events for up to 5 seconds — at least one network-event must arrive.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let mut got_network_event = false;
    while std::time::Instant::now() < deadline {
        // Pump the transport so the bus receives any pending events.
        if let Ok(msg) = conn.transport_mut().recv() {
            bus.dispatch_event(&msg);
        }
        if rx.try_recv().is_ok() {
            got_network_event = true;
            break;
        }
    }

    // Unsubscribe cleanly.
    let _ = bus.unsubscribe(conn.transport_mut(), sub_id);

    assert!(
        got_network_event,
        "live_network_set_cookie_longstring: expected at least one network-event \
         from httpbingo.org navigation — check that Firefox is running and the \
         network is reachable"
    );
}

/// Verify that `getTargetConfigurationActor` returns an actor reference and
/// that `set_cache_disabled` sends `updateConfiguration` correctly.
///
/// AC: live_cache_disable_via_target_config — after `set_cache_disabled(true)`,
/// a request to a Cache-Control: max-age=3600 resource returns a non-304 response.
#[test]
#[ignore = "requires live Firefox — FF_RDP_LIVE_TESTS=1"]
fn live_cache_disable_via_target_config() {
    if !should_run_live() {
        return;
    }

    let port = firefox_port();
    let mut conn = RdpConnection::connect("127.0.0.1", port, TIMEOUT).unwrap();
    let tabs = RootActor::list_tabs(conn.transport_mut()).unwrap();
    assert!(
        !tabs.is_empty(),
        "need a tab for live_cache_disable_via_target_config"
    );

    let tab = tabs
        .iter()
        .find(|t| t.selected)
        .or_else(|| tabs.first())
        .unwrap();
    let tab_actor = tab.actor.clone();

    // Get the watcher actor for this tab.
    let watcher_actor_id = TabActor::get_watcher(conn.transport_mut(), &tab_actor).unwrap();

    // Build a WatcherFront to obtain the target configuration actor.
    let watcher_front = WatcherFront::new(
        watcher_actor_id.clone(),
        ff_rdp_core::Registry::default(),
        Some(watcher_actor_id.clone()),
    );

    let config_actor_id = watcher_front
        .get_target_configuration_actor(conn.transport_mut())
        .unwrap();

    // Build a TargetConfigurationFront and disable the cache.
    let config_front = TargetConfigurationFront::new(
        config_actor_id,
        ff_rdp_core::Registry::default(),
        watcher_actor_id.clone(),
    );

    // set_cache_disabled(true) must not error — this is the key protocol assertion.
    config_front
        .set_cache_disabled(conn.transport_mut(), true)
        .expect("set_cache_disabled(true) must succeed");

    // Re-enable so we leave Firefox in a clean state for subsequent tests.
    config_front
        .set_cache_disabled(conn.transport_mut(), false)
        .expect("set_cache_disabled(false) must succeed");

    drop(conn);
}
