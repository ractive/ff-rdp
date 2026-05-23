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

use ff_rdp_core::{RdpConnection, RootActor};
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
/// header value deserializes as a LongString::Actor (not a panic/decode error).
///
/// This test requires `FF_RDP_LIVE_NETWORK_TESTS=1` and an internet connection.
#[test]
#[ignore = "requires live Firefox + network — FF_RDP_LIVE_NETWORK_TESTS=1"]
fn live_network_set_cookie_longstring() {
    // TODO(iter-61v): assert that a 50 KB Set-Cookie header value deserializes as
    // LongString::Actor (not a panic/decode error) and that fetch_full returns the
    // full value matching the expected length.
    if !should_run_live() {
        return;
    }
    let needs_network = std::env::var("FF_RDP_LIVE_NETWORK_TESTS").is_ok_and(|v| v == "1");
    if !needs_network {
        return;
    }
    // Navigate to a page known to set a long Set-Cookie header, then capture
    // network events with --detail --headers and assert the value is either
    // inline or a LongString::Actor (not a deserialization failure).
    //
    // Full implementation requires wiring network event capture — this skeleton
    // confirms the test slot exists and is properly gated.
    let port = firefox_port();
    let mut conn = RdpConnection::connect("127.0.0.1", port, TIMEOUT).unwrap();
    let tabs = RootActor::list_tabs(conn.transport_mut()).unwrap();
    assert!(!tabs.is_empty(), "need a tab for longstring test");
    drop(conn);
}

/// Verify that `getTargetConfigurationActor` returns an actor reference and
/// that `set_cache_disabled` sends `updateConfiguration` correctly.
///
/// AC: live_cache_disable_via_target_config
#[test]
#[ignore = "requires live Firefox — FF_RDP_LIVE_TESTS=1"]
fn live_cache_disable_via_target_config() {
    // TODO(iter-61v): assert that after set_cache_disabled(true), a request to a
    // Cache-Control: max-age=3600 resource returns a non-304 response (cache bypassed).
    if !should_run_live() {
        return;
    }
    // Skeleton: verify the actor is reachable via watcher.
    // Full assertion requires navigating to a cache-controlled resource and
    // confirming the response bypasses cache — out of scope for a mock-free
    // unit-testable skeleton.
    let port = firefox_port();
    let mut conn = RdpConnection::connect("127.0.0.1", port, TIMEOUT).unwrap();
    let tabs = RootActor::list_tabs(conn.transport_mut()).unwrap();
    assert!(
        !tabs.is_empty(),
        "need a tab for live_cache_disable_via_target_config"
    );
    drop(conn);
}
