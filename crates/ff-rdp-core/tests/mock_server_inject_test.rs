//! Snapshot test for `MockServerHandle::inject_watcher_resource`.
//!
//! Verifies that the mock server can push unsolicited `resources-available-array`
//! packets to a connected client while `serve_one` is running in a worker thread.

mod support;

use std::time::Duration;

use ff_rdp_core::transport::RdpTransport;
use serde_json::json;
use support::MockRdpServer;

const TIMEOUT: Duration = Duration::from_secs(5);
const WATCHER_ACTOR: &str = "watcher1";

/// Connect a raw transport to the mock server so we can read arbitrary frames,
/// including injected ones that don't follow the request/response pattern.
fn connect_raw(port: u16) -> RdpTransport {
    RdpTransport::connect_raw("127.0.0.1", port, TIMEOUT).expect("connect_raw")
}

/// `mock_server_inject_watcher_resource`:
/// Spawns the mock server, connects a client, injects three resource types
/// (`network-event`, `console-message`, `document-event`), and asserts that
/// each packet arrives with the correct `resources-available-array` envelope.
#[test]
fn mock_server_inject_watcher_resource() {
    let server = MockRdpServer::new()
        .with_greeting(json!({"from":"root","applicationType":"browser","traits":{}}));

    let handle = server.handle();
    let port = handle.port;

    // Move the server into a worker thread.
    let server_thread = std::thread::spawn(move || server.serve_one());

    // Connect a raw transport (reads the greeting, then we can read arbitrary frames).
    let mut transport = connect_raw(port);

    // Consume the greeting.
    let greeting = transport.recv().expect("greeting");
    assert_eq!(greeting["from"], "root", "unexpected greeting: {greeting}");

    // Inject three different resource types.
    let resource_types = ["network-event", "console-message", "document-event"];
    for rt in &resource_types {
        handle.inject_watcher_resource(
            WATCHER_ACTOR,
            rt,
            &[json!({"resourceType": rt, "detail": "payload"})],
        );
    }

    // Read three frames and verify each one.
    for expected_type in &resource_types {
        transport
            .set_read_timeout(Some(TIMEOUT))
            .expect("set_read_timeout");

        let frame = transport.recv().unwrap_or_else(|e| {
            panic!("failed to receive injected frame for {expected_type}: {e}")
        });

        assert_eq!(
            frame["from"].as_str(),
            Some(WATCHER_ACTOR),
            "expected from={WATCHER_ACTOR}, got: {frame}"
        );
        assert_eq!(
            frame["type"].as_str(),
            Some("resources-available-array"),
            "expected type=resources-available-array for {expected_type}, got: {frame}"
        );

        // The array field should be [[resource_type, [payload...]]]
        let array = frame["array"]
            .as_array()
            .unwrap_or_else(|| panic!("missing array field in: {frame}"));
        assert!(
            !array.is_empty(),
            "array must be non-empty for {expected_type}"
        );

        let first_pair = array[0]
            .as_array()
            .unwrap_or_else(|| panic!("first array element must be an array in: {frame}"));
        assert_eq!(
            first_pair[0].as_str(),
            Some(*expected_type),
            "resource_type mismatch — expected {expected_type}, got: {frame}"
        );
    }

    // Drop the transport (client disconnect) so serve_one exits.
    drop(transport);
    server_thread.join().expect("server thread panicked");
}

/// `mock_server_inject_event_arbitrary`:
/// Verifies that `inject_event` sends an arbitrary packet as-is (no wrapping).
#[test]
fn mock_server_inject_event_arbitrary() {
    let server = MockRdpServer::new()
        .with_greeting(json!({"from":"root","applicationType":"browser","traits":{}}));

    let handle = server.handle();
    let port = handle.port;

    let server_thread = std::thread::spawn(move || server.serve_one());

    let mut transport = connect_raw(port);

    // Consume greeting.
    transport.recv().expect("greeting");

    // Inject an arbitrary packet.
    handle.inject_event(json!({
        "from": "conn0/watcher1",
        "type": "target-available-form",
        "target": {"actor": "conn0/target1"}
    }));

    transport
        .set_read_timeout(Some(TIMEOUT))
        .expect("set_read_timeout");
    let frame = transport.recv().expect("injected event");

    assert_eq!(frame["type"].as_str(), Some("target-available-form"));
    assert_eq!(frame["from"].as_str(), Some("conn0/watcher1"));

    drop(transport);
    server_thread.join().expect("server thread panicked");
}
