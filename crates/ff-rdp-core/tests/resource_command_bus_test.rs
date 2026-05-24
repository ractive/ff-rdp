//! Mock-server-driven unit test for `ResourceCommand` bus.
//!
//! Acceptance criterion: AC5 —
//! "Mock-server-driven unit test: bus correctly dedupes subscribers,
//!  fans out events, and unsubscribes on last drop."
//!
//! This test drives a `MockRdpServer` that captures all `watchResources` /
//! `unwatchResources` wire calls and injects `resources-available-array` events.
//! It verifies:
//!   1. Two in-process subscribers for the same type produce exactly ONE
//!      `watchResources` call on the wire.
//!   2. Events are fanned out to all subscribers that requested the matching type.
//!   3. The final `unsubscribe` (ref-count → 0) sends exactly ONE
//!      `unwatchResources` call on the wire.
//!   4. A subscriber that doesn't request a type doesn't receive its events.
//!   5. Dead subscriber channels (dropped receivers) are cleaned up without error.

mod support;

use std::time::Duration;

use ff_rdp_core::transport::RdpTransport;
use ff_rdp_core::{Resource, ResourceCommand, ResourceType};
use serde_json::json;
use support::MockRdpServer;

const TIMEOUT: Duration = Duration::from_secs(5);
const WATCHER: &str = "conn0/watcher1";

/// Build a mock server that responds OK to `watchResources` and `unwatchResources`.
fn make_server() -> MockRdpServer {
    MockRdpServer::new()
        .with_greeting(json!({"from": "root", "applicationType": "browser", "traits": {}}))
        .on(
            "watchResources",
            json!({"from": WATCHER, "resourceTypes": []}),
        )
        .on(
            "unwatchResources",
            json!({"from": WATCHER, "resourceTypes": []}),
        )
        .on("watchTargets", json!({"from": WATCHER}))
        .on("unwatchTargets", json!({"from": WATCHER}))
}

fn connect(port: u16) -> RdpTransport {
    let mut t = RdpTransport::connect_raw("127.0.0.1", port, TIMEOUT).expect("connect");
    t.set_read_timeout(Some(TIMEOUT)).expect("set_read_timeout");
    // Consume greeting.
    t.recv().expect("greeting");
    t
}

/// AC5.1 — Two subscribers for the same type → ONE `watchResources` wire call.
#[test]
fn bus_deduplicates_watch_resources_calls() {
    let server = make_server();
    let handle = server.handle();
    let server_thread = std::thread::spawn(move || server.serve_one());

    let mut transport = connect(handle.port);
    let mut bus = ResourceCommand::new(ff_rdp_core::ActorId::from(WATCHER));

    // First subscriber — should trigger watchResources.
    let (sub_id_a, _rx_a) = bus
        .subscribe(&mut transport, &[ResourceType::NetworkEvent])
        .expect("subscribe A");
    assert_eq!(
        bus.ref_count(ResourceType::NetworkEvent),
        1,
        "ref-count should be 1 after first subscribe"
    );

    // Second subscriber — same type, should NOT trigger watchResources again.
    let (_sub_id_b, _rx_b) = bus
        .subscribe(&mut transport, &[ResourceType::NetworkEvent])
        .expect("subscribe B");
    assert_eq!(
        bus.ref_count(ResourceType::NetworkEvent),
        2,
        "ref-count should be 2 after second subscribe"
    );
    assert_eq!(
        bus.subscriber_count(),
        2,
        "bus should have 2 in-process subscribers"
    );

    // Unsubscribe only the first subscriber; ref-count goes to 1 (not zero,
    // so no `unwatchResources` wire call is expected for this test).
    bus.unsubscribe(&mut transport, sub_id_a)
        .expect("unsubscribe A");
    assert_eq!(
        bus.ref_count(ResourceType::NetworkEvent),
        1,
        "ref-count should be 1 after first unsubscribe"
    );

    // Drop the bus to close transport; server will EOF.
    drop(bus);
    drop(transport);

    let requests = server_thread.join().expect("server thread");
    let watch_calls: Vec<_> = requests
        .iter()
        .filter(|r| r["type"] == "watchResources")
        .collect();
    assert_eq!(
        watch_calls.len(),
        1,
        "expected exactly 1 watchResources call on the wire, got: {watch_calls:?}"
    );
}

/// AC5.2 — `watchResources` + `unwatchResources` are symmetric: last unsubscribe
/// sends `unwatchResources`.
#[test]
fn bus_sends_unwatch_when_last_subscriber_drops() {
    let server = make_server();
    let handle = server.handle();
    let server_thread = std::thread::spawn(move || server.serve_one());

    let mut transport = connect(handle.port);
    let mut bus = ResourceCommand::new(ff_rdp_core::ActorId::from(WATCHER));

    let (id_a, _rx_a) = bus
        .subscribe(&mut transport, &[ResourceType::NetworkEvent])
        .expect("subscribe A");
    let (id_b, _rx_b) = bus
        .subscribe(&mut transport, &[ResourceType::NetworkEvent])
        .expect("subscribe B");

    bus.unsubscribe(&mut transport, id_a)
        .expect("unsubscribe A");
    // ref-count still 1 — no unwatchResources yet.
    bus.unsubscribe(&mut transport, id_b)
        .expect("unsubscribe B");
    // ref-count now 0 — unwatchResources should fire.

    drop(bus);
    drop(transport);

    let requests = server_thread.join().expect("server thread");
    let unwatch_calls: Vec<_> = requests
        .iter()
        .filter(|r| r["type"] == "unwatchResources")
        .collect();
    assert_eq!(
        unwatch_calls.len(),
        1,
        "expected exactly 1 unwatchResources call, got: {unwatch_calls:?}"
    );
}

/// AC5.3 — Events are fanned out to all subscribers that requested the type;
/// subscribers that didn't request a type receive nothing.
#[test]
fn bus_fans_out_to_matching_subscribers_only() {
    let server = make_server();
    let handle = server.handle();
    let server_thread = std::thread::spawn(move || server.serve_one());

    let mut transport = connect(handle.port);
    let mut bus = ResourceCommand::new(ff_rdp_core::ActorId::from(WATCHER));

    // Subscriber A wants network events.
    let (_id_a, rx_a) = bus
        .subscribe(&mut transport, &[ResourceType::NetworkEvent])
        .expect("subscribe A");

    // Subscriber B wants console messages only.
    let (_id_b, rx_b) = bus
        .subscribe(&mut transport, &[ResourceType::ConsoleMessage])
        .expect("subscribe B");

    // Subscriber C wants both.
    let (_id_c, rx_c) = bus
        .subscribe(
            &mut transport,
            &[ResourceType::NetworkEvent, ResourceType::ConsoleMessage],
        )
        .expect("subscribe C");

    // Inject a network-event into the bus directly (no server involvement needed
    // for dispatch — we call dispatch_event with a hand-crafted packet).
    let net_packet = json!({
        "from": WATCHER,
        "type": "resources-available-array",
        "array": [
            ["network-event", [{
                "actor": "conn0/netEvent1",
                "method": "GET",
                "url": "https://example.com/",
                "isXHR": false,
                "cause": {"type": "document"},
                "startedDateTime": "2026-01-01T00:00:00Z",
                "timeStamp": 1000.0,
                "resourceId": 1_u64
            }]]
        ]
    });
    bus.dispatch_event(&net_packet);

    // Inject a console-message.
    let console_packet = json!({
        "from": WATCHER,
        "type": "resources-available-array",
        "array": [
            ["console-message", [{
                "resourceId": 42_u64,
                "message": {
                    "arguments": ["hello"],
                    "level": "log",
                    "filename": "test.js",
                    "lineNumber": 1,
                    "columnNumber": 0,
                    "timeStamp": 2000.0
                }
            }]]
        ]
    });
    bus.dispatch_event(&console_packet);

    // Subscriber A: should have received exactly 1 network event.
    let a_events: Vec<std::sync::Arc<Resource>> = rx_a.try_iter().collect();
    assert_eq!(a_events.len(), 1, "subscriber A should get 1 event");
    assert!(
        matches!(a_events[0].as_ref(), Resource::NetworkEvent(_)),
        "subscriber A should get a NetworkEvent"
    );

    // Subscriber B: should have received exactly 1 console message.
    let b_events: Vec<std::sync::Arc<Resource>> = rx_b.try_iter().collect();
    assert_eq!(b_events.len(), 1, "subscriber B should get 1 event");
    assert!(
        matches!(b_events[0].as_ref(), Resource::ConsoleMessage(_)),
        "subscriber B should get a ConsoleMessage"
    );

    // Subscriber C: should have received both.
    let c_events: Vec<std::sync::Arc<Resource>> = rx_c.try_iter().collect();
    assert_eq!(c_events.len(), 2, "subscriber C should get 2 events");

    drop(bus);
    drop(transport);
    let _ = server_thread.join();
}

/// AC5.4 — Dead receiver (dropped) is cleaned up lazily without panic.
#[test]
fn bus_handles_dead_receiver_gracefully() {
    let server = make_server();
    let handle = server.handle();
    let server_thread = std::thread::spawn(move || server.serve_one());

    let mut transport = connect(handle.port);
    let mut bus = ResourceCommand::new(ff_rdp_core::ActorId::from(WATCHER));

    let (_id, rx) = bus
        .subscribe(&mut transport, &[ResourceType::NetworkEvent])
        .expect("subscribe");

    // Drop the receiver — channel is now dead from subscriber's side.
    drop(rx);

    // Dispatching an event to a dead channel must not panic.
    let packet = json!({
        "type": "resources-available-array",
        "array": [
            ["network-event", [{
                "actor": "conn0/netEvent1",
                "method": "GET",
                "url": "https://example.com/",
                "isXHR": false,
                "cause": {"type": "document"},
                "startedDateTime": "2026-01-01T00:00:00Z",
                "timeStamp": 1000.0,
                "resourceId": 1_u64
            }]]
        ]
    });
    bus.dispatch_event(&packet);
    // If we reach here without panicking, the test passes.

    drop(bus);
    drop(transport);
    let _ = server_thread.join();
}

/// `resource_command_unwatch_on_drop` (iter-71 Theme A AC):
/// subscribe → drop receiver → dispatch event to trigger dead-channel prune →
/// call `gc()` → assert exactly one `unwatchResources` packet on the wire.
#[test]
fn resource_command_unwatch_on_drop() {
    let server = make_server();
    let handle = server.handle();
    let server_thread = std::thread::spawn(move || server.serve_one());

    let mut transport = connect(handle.port);
    let mut bus = ResourceCommand::new(ff_rdp_core::ActorId::from(WATCHER));

    // Subscribe so the wire gets a `watchResources`.
    let (_id, rx) = bus
        .subscribe(&mut transport, &[ResourceType::NetworkEvent])
        .expect("subscribe");

    // Drop the receiver — channel becomes dead.
    drop(rx);

    // Dispatch an event so dead-channel pruning fires inside `dispatch_event`.
    let packet = serde_json::json!({
        "type": "resources-available-array",
        "array": [
            ["network-event", [{
                "actor": "conn0/netEvent1",
                "method": "GET",
                "url": "https://example.com/",
                "isXHR": false,
                "cause": {"type": "document"},
                "startedDateTime": "2026-01-01T00:00:00Z",
                "timeStamp": 1000.0,
                "resourceId": 1_u64
            }]]
        ]
    });
    bus.dispatch_event(&packet);

    assert_eq!(
        bus.pending_unwatch_count(),
        1,
        "pending_unwatch should be non-zero after dead-channel prune"
    );

    // gc() flushes the pending `unwatchResources` wire call.
    bus.gc(&mut transport).expect("gc");

    assert_eq!(
        bus.pending_unwatch_count(),
        0,
        "pending_unwatch should be empty after gc()"
    );

    drop(bus);
    drop(transport);

    let requests = server_thread.join().expect("server thread");
    let unwatch_calls: Vec<_> = requests
        .iter()
        .filter(|r| r["type"] == "unwatchResources")
        .collect();
    assert_eq!(
        unwatch_calls.len(),
        1,
        "gc() should have sent exactly 1 unwatchResources for the dead subscriber: {unwatch_calls:?}"
    );
}

/// AC5.5 — Multiple resource types in one `subscribe` call produce one wire call
/// covering all requested types.
#[test]
fn bus_subscribe_multiple_types_in_one_call() {
    let server = make_server();
    let handle = server.handle();
    let server_thread = std::thread::spawn(move || server.serve_one());

    let mut transport = connect(handle.port);
    let mut bus = ResourceCommand::new(ff_rdp_core::ActorId::from(WATCHER));

    let types = &[ResourceType::NetworkEvent, ResourceType::ConsoleMessage];
    let (_id, _rx) = bus.subscribe(&mut transport, types).expect("subscribe");

    assert_eq!(bus.ref_count(ResourceType::NetworkEvent), 1);
    assert_eq!(bus.ref_count(ResourceType::ConsoleMessage), 1);

    drop(bus);
    drop(transport);

    let requests = server_thread.join().expect("server thread");
    let watch_calls: Vec<_> = requests
        .iter()
        .filter(|r| r["type"] == "watchResources")
        .collect();
    assert_eq!(
        watch_calls.len(),
        1,
        "one subscribe call with two types should produce 1 watchResources: {watch_calls:?}"
    );
}
