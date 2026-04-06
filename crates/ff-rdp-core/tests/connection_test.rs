mod support;

use std::time::Duration;

use ff_rdp_core::{ProtocolError, RdpConnection, RootActor};
use serde_json::json;
use support::{MockRdpServer, load_fixture};

const TIMEOUT: Duration = Duration::from_secs(5);

// ---------------------------------------------------------------------------
// RdpConnection::connect — greeting validation
// ---------------------------------------------------------------------------

#[test]
fn connect_succeeds_with_valid_greeting() {
    let server = MockRdpServer::new();
    let port = server.port();
    let server_thread = std::thread::spawn(move || server.serve_one());

    let result = RdpConnection::connect("127.0.0.1", port, TIMEOUT);
    assert!(result.is_ok(), "expected Ok, got {result:?}");

    // Drive the server to completion (it will exit on client drop).
    drop(result);
    server_thread.join().unwrap();
}

#[test]
fn connect_fails_with_wrong_application_type() {
    let server = MockRdpServer::new().with_greeting(json!({
        "from": "root",
        "applicationType": "webide",
        "traits": {}
    }));
    let port = server.port();
    let server_thread = std::thread::spawn(move || server.serve_one());

    let err = RdpConnection::connect("127.0.0.1", port, TIMEOUT).unwrap_err();

    assert!(
        matches!(err, ProtocolError::InvalidPacket(_)),
        "expected InvalidPacket, got {err:?}"
    );

    // The server task may complete with an error because the client closed
    // the connection without sending any request — that is fine.
    let _ = server_thread.join();
}

#[test]
fn connect_fails_with_missing_application_type() {
    let server = MockRdpServer::new().with_greeting(json!({"from": "root"}));
    let port = server.port();
    let server_thread = std::thread::spawn(move || server.serve_one());

    let err = RdpConnection::connect("127.0.0.1", port, TIMEOUT).unwrap_err();

    assert!(
        matches!(err, ProtocolError::InvalidPacket(_)),
        "expected InvalidPacket, got {err:?}"
    );

    let _ = server_thread.join();
}

// ---------------------------------------------------------------------------
// RootActor::list_tabs — happy paths
// ---------------------------------------------------------------------------

#[test]
fn list_tabs_returns_parsed_tabs() {
    let list_tabs_response = load_fixture("list_tabs_response.json");

    let server = MockRdpServer::new().on("listTabs", list_tabs_response);
    let port = server.port();
    let server_thread = std::thread::spawn(move || server.serve_one());

    let mut conn = RdpConnection::connect("127.0.0.1", port, TIMEOUT).unwrap();

    let tabs = RootActor::list_tabs(conn.transport_mut()).unwrap();

    assert_eq!(tabs.len(), 2, "expected 2 tabs");

    let first = &tabs[0];
    assert_eq!(first.actor.as_ref(), "server1.conn0.tabDescriptor1");
    assert_eq!(first.title, "Example Domain");
    assert_eq!(first.url, "https://example.com/");
    assert!(first.selected);
    assert_eq!(first.browsing_context_id, Some(22));

    let second = &tabs[1];
    assert_eq!(second.actor.as_ref(), "server1.conn0.tabDescriptor2");
    assert_eq!(second.title, "Rust Programming Language");
    assert_eq!(second.url, "https://www.rust-lang.org/");
    assert!(!second.selected);
    assert_eq!(second.browsing_context_id, Some(25));

    drop(conn);
    let requests = server_thread.join().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0]["type"], "listTabs");
}

#[test]
fn list_tabs_with_fixture_greeting() {
    let handshake = load_fixture("handshake.json");
    let list_tabs_response = load_fixture("list_tabs_response.json");

    let server = MockRdpServer::new()
        .with_greeting(handshake)
        .on("listTabs", list_tabs_response);
    let port = server.port();
    let server_thread = std::thread::spawn(move || server.serve_one());

    let mut conn = RdpConnection::connect("127.0.0.1", port, TIMEOUT).unwrap();

    let tabs = RootActor::list_tabs(conn.transport_mut()).unwrap();
    assert_eq!(tabs.len(), 2);

    drop(conn);
    server_thread.join().unwrap();
}

#[test]
fn list_tabs_returns_empty_vec_for_empty_tabs() {
    let server = MockRdpServer::new().on(
        "listTabs",
        json!({"from": "root", "tabs": [], "selected": 0}),
    );
    let port = server.port();
    let server_thread = std::thread::spawn(move || server.serve_one());

    let mut conn = RdpConnection::connect("127.0.0.1", port, TIMEOUT).unwrap();

    let tabs = RootActor::list_tabs(conn.transport_mut()).unwrap();
    assert!(tabs.is_empty(), "expected empty tab list");

    drop(conn);
    server_thread.join().unwrap();
}

// ---------------------------------------------------------------------------
// RootActor::get_root
// ---------------------------------------------------------------------------

#[test]
fn get_root_returns_actor_metadata() {
    let server = MockRdpServer::new().on(
        "getRoot",
        json!({
            "from": "root",
            "preferenceActor": "server1.conn0.preferenceActor1",
            "deviceActor": "server1.conn0.deviceActor1",
            "addonsActor": "server1.conn0.addonsActor1"
        }),
    );
    let port = server.port();
    let server_thread = std::thread::spawn(move || server.serve_one());

    let mut conn = RdpConnection::connect("127.0.0.1", port, TIMEOUT).unwrap();

    let root = RootActor::get_root(conn.transport_mut()).unwrap();
    assert_eq!(root["preferenceActor"], "server1.conn0.preferenceActor1");
    assert_eq!(root["deviceActor"], "server1.conn0.deviceActor1");

    drop(conn);
    server_thread.join().unwrap();
}

// ---------------------------------------------------------------------------
// Connection error paths
// ---------------------------------------------------------------------------

#[test]
fn connect_to_closed_port_fails_with_connection_error() {
    // Bind a listener, grab its port, then drop it so nothing is listening.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let err = RdpConnection::connect("127.0.0.1", port, TIMEOUT).unwrap_err();

    assert!(
        matches!(
            err,
            ProtocolError::ConnectionFailed(_) | ProtocolError::Timeout
        ),
        "expected ConnectionFailed or Timeout, got {err:?}"
    );
}

#[test]
fn connect_times_out_when_server_does_not_send_greeting() {
    // Accept the TCP connection but never write the greeting, so the client
    // times out waiting for it.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    let silent_server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        // Block until the client disconnects (EOF).
        let mut buf = [0u8; 1];
        let _ = std::io::Read::read(&mut &stream, &mut buf);
    });

    let short_timeout = Duration::from_millis(100);
    let err = RdpConnection::connect("127.0.0.1", port, short_timeout).unwrap_err();

    assert!(
        matches!(err, ProtocolError::Timeout),
        "expected Timeout, got {err:?}"
    );

    let _ = silent_server.join();
}
