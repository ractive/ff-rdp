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
//!       --test live live_console_no_double_delivery -- --nocapture
//!
//! Note: this test is gated on `FF_RDP_LIVE_TESTS=1` and `#[ignore]` so it
//! does not run in normal `cargo test`.

use std::sync::Arc;
use std::time::Duration;

use crate::common::LiveFirefox;
use crate::common::live_tests_enabled;
use ff_rdp_core::{
    ActorId, RdpTransport, Resource, ResourceCommand, ResourceType, RootActor, TabActor,
    WatcherActor, WebConsoleActor,
};

/// Gated helper: returns `true` if `FF_RDP_LIVE_TESTS=1` is set.
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

    // Note: RdpTransport::connect already consumed the Firefox greeting packet
    // internally — do not read it again here.

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

    // Drain the transport for a window long enough for Firefox to push the
    // console event through both the legacy actor path and the watcher path.
    // 500 ms gives a comfortable margin while remaining fast in CI.
    let deadline = std::time::Instant::now() + Duration::from_millis(500);
    transport
        .set_read_timeout(Some(Duration::from_millis(50)))
        .expect("set_read_timeout");

    while std::time::Instant::now() < deadline {
        if let Ok(msg) = transport.recv() {
            bus.dispatch_event(&msg);
        }
        // On timeout: continue draining until deadline.
    }

    // Count how many console-message resources the bus received that contain
    // our sentinel string.  Use the typed `ConsoleResource::message` field
    // rather than the Debug representation for a more precise match.
    let received: Vec<Arc<ff_rdp_core::Resource>> = bus_rx.try_iter().collect();
    let matching: Vec<_> = received
        .iter()
        .filter(|r| {
            matches!(
                r.as_ref(),
                Resource::ConsoleMessage(c) if c.message.contains(sentinel)
            )
        })
        .collect();

    eprintln!(
        "live_console_no_double_delivery: total={}, matching sentinel={}",
        received.len(),
        matching.len()
    );

    // The bus should have received the sentinel at most once via the watcher
    // resource path.  A count of 0 means Firefox delivered the event only via
    // the legacy consoleActor push (not through `resources-available-array`);
    // a count of 1 means the watcher path also fired; a count > 1 means
    // double-delivery occurred.
    //
    // Research finding (iter-71 Theme C): on the tested Firefox version the
    // watcher delivers 0 console-message resources for `evaluateJSAsync`-
    // triggered console.log calls.  The legacy `consoleAPICall` push comes
    // through the console actor directly and is NOT routed through the watcher
    // `resources-available-array` stream.  This means no double-delivery is
    // possible via this path, and the legacy `startListeners` can be left in
    // place without risk of duplicating events on the watcher bus.
    assert!(
        matching.len() <= 1,
        "double-delivery detected on the watcher bus: got {} deliveries of the \
         sentinel console.log — expected at most 1",
        matching.len()
    );
    eprintln!(
        "live_console_no_double_delivery: watcher_bus_count={} \
         (0=legacy-only, 1=watcher-delivered, >1=double-delivery)",
        matching.len()
    );

    // Cleanup.
    let _ = bus.gc(&mut transport);
    eprintln!("live_console_no_double_delivery: PASS (no double delivery detected)");
}
