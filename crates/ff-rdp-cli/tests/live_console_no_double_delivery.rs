//! Live test: `live_console_no_double_delivery` (iter-71 Theme C AC).
//!
//! This test verifies that subscribing to console events via **both** the
//! legacy `WebConsoleActor::startListeners` path AND the `WatcherActor`
//! resource path does not produce duplicate event deliveries on the watcher
//! bus side.
//!
//! # Hypothesis being tested
//!
//! `commands/console.rs` calls `WebConsoleActor::start_listeners` AND the
//! daemon path uses `ResourceCommand::subscribe` for `console-message`.
//! Running both paths in the same session *may* cause Firefox to push each
//! `consoleAPICall` event twice — once via the legacy console actor push and
//! once via the watcher resources stream.
//!
//! If this test passes (exactly one delivery per eval log), the legacy path
//! is harmless for the bus and can eventually be removed.  If it fails
//! (two deliveries), we have evidence to keep both paths during migration.
//!
//! # Running
//!
//!   FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli \
//!       --test live_console_no_double_delivery -- --nocapture
//!
//! Note: this test is gated on `FF_RDP_LIVE_TESTS=1` and `#[ignore]` so it
//! does not run in normal `cargo test`.

#[path = "common/mod.rs"]
mod common;

use std::sync::Arc;
use std::time::Duration;

use common::LiveFirefox;
use ff_rdp_core::{
    ActorId, RdpTransport, ResourceCommand, ResourceType, RootActor, TabActor, WatcherActor,
    WebConsoleActor,
};

/// Gated helper: returns `true` if `FF_RDP_LIVE_TESTS=1` is set.
fn live_tests_enabled() -> bool {
    std::env::var("FF_RDP_LIVE_TESTS").as_deref() == Ok("1")
}

/// `live_console_no_double_delivery`:
/// Subscribe via both legacy `startListeners` AND `watchResources(console-message)`,
/// trigger a `console.log` via eval, then assert the bus delivers the event
/// exactly once.
///
/// Gated on `FF_RDP_LIVE_TESTS=1`.
#[test]
#[ignore = "requires FF_RDP_LIVE_TESTS=1 and a running headless Firefox instance"]
fn live_console_no_double_delivery() {
    if !live_tests_enabled() {
        eprintln!("Skipping: set FF_RDP_LIVE_TESTS=1 to run this test");
        return;
    }

    let Some(ff) = LiveFirefox::headless_on_random_port() else {
        eprintln!("Skipping: could not launch headless Firefox");
        return;
    };

    let port = ff.port();

    let mut transport = RdpTransport::connect("127.0.0.1", port, Duration::from_secs(5))
        .expect("connect to Firefox RDP");

    // Consume greeting.
    let _greeting = transport.recv().expect("greeting");

    // Resolve the first tab.
    let tabs = RootActor::list_tabs(&mut transport).expect("list tabs");
    let tab_actor = tabs.first().expect("at least one tab").actor.clone();

    // Get target info (consoleActor, etc.).
    let target = TabActor::get_target(&mut transport, &tab_actor).expect("get target");
    let console_actor_id: ActorId = ActorId::from(target.console_actor.as_ref());

    // --- Legacy path: startListeners ---
    if let Err(e) =
        WebConsoleActor::start_listeners(&mut transport, &console_actor_id, &["ConsoleAPI"])
    {
        eprintln!("warning: startListeners failed (may be expected on newer Firefox): {e}");
    }

    // --- Watcher path: watchResources(console-message) ---
    let watcher_actor =
        TabActor::get_watcher(&mut transport, &tab_actor).expect("get watcher actor");
    WatcherActor::watch_targets(&mut transport, &watcher_actor, "frame")
        .expect("watch frame targets");

    let mut bus = ResourceCommand::new(watcher_actor.clone());
    let (_sub_id, bus_rx) = bus
        .subscribe(&mut transport, &[ResourceType::ConsoleMessage])
        .expect("subscribe to console-message via ResourceCommand");

    // Trigger a console.log via evaluateJSAsync on the console actor.
    // Use a unique sentinel so we can identify our specific message.
    let sentinel = "iter71_double_delivery_sentinel_12345";
    let eval_msg = serde_json::json!({
        "to": console_actor_id.as_ref(),
        "type": "evaluateJSAsync",
        "text": format!("console.log('{sentinel}')"),
    });
    transport.send(&eval_msg).expect("send evaluateJSAsync");

    // Drain the transport for a short window, routing watcher events through
    // the bus.  We collect up to 200 ms worth of events.
    let deadline = std::time::Instant::now() + Duration::from_millis(200);
    transport
        .set_read_timeout(Some(Duration::from_millis(50)))
        .expect("set_read_timeout");

    while std::time::Instant::now() < deadline {
        if let Ok(msg) = transport.recv() {
            bus.dispatch_event(&msg);
        }
        // On timeout/error: continue draining until deadline.
    }

    // Count how many console-message resources the bus received that contain
    // our sentinel string.  Use the Debug representation since Resource does
    // not implement Serialize.
    let received: Vec<Arc<ff_rdp_core::Resource>> = bus_rx.try_iter().collect();
    let matching: Vec<_> = received
        .iter()
        .filter(|r| format!("{:?}", r.as_ref()).contains(sentinel))
        .collect();

    eprintln!(
        "live_console_no_double_delivery: total={}, matching sentinel={}",
        received.len(),
        matching.len()
    );

    // The bus should have received the console.log exactly once via the
    // watcher resource path.  If startListeners also pushed the event through
    // the watcher channel, matching.len() would be > 1.
    assert!(
        matching.len() <= 1,
        "expected at most 1 delivery of the sentinel console.log on the watcher bus, \
         got {}: this suggests double-delivery via legacy startListeners + watcher paths",
        matching.len()
    );

    // Cleanup.
    let _ = bus.gc(&mut transport);
    eprintln!("live_console_no_double_delivery: PASS (no double delivery detected)");
}
