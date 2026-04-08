mod support;

use std::time::Duration;

use ff_rdp_core::{RdpConnection, StorageActor};
use serde_json::json;
use support::MockRdpServer;

const TIMEOUT: Duration = Duration::from_secs(5);

/// Verify that `list_cookies` sends `options.sessionString` in the
/// `getStoreObjects` request — Firefox 149+ crashes without it.
#[test]
fn list_cookies_sends_session_string_in_get_store_objects() {
    let tab_actor = "server1.conn0.tabDescriptor1";
    let watcher_actor = "server1.conn0.watcher2";
    let cookie_actor = "server1.conn0.cookies3";

    let server = MockRdpServer::new()
        .on(
            "getWatcher",
            json!({
                "from": tab_actor,
                "actor": watcher_actor,
            }),
        )
        .on(
            "watchResources",
            json!({
                "from": watcher_actor,
                "type": "resources-available-array",
                "array": [["cookies", [{
                    "actor": cookie_actor,
                    "hosts": { "https://example.com": [] },
                    "resourceId": "cookies-12345",
                    "traits": {}
                }]]]
            }),
        )
        .on(
            "getStoreObjects",
            json!({
                "from": cookie_actor,
                "data": [{
                    "name": "test_cookie",
                    "value": "abc",
                    "host": "example.com",
                    "path": "/",
                    "expires": 0,
                    "size": 11,
                    "isHttpOnly": false,
                    "isSecure": false,
                    "sameSite": "",
                    "hostOnly": true,
                    "lastAccessed": 0.0,
                    "creationTime": 0.0
                }],
                "offset": 0,
                "total": 1
            }),
        )
        .on("unwatchResources", json!({ "from": watcher_actor }));

    let port = server.port();
    let server_thread = std::thread::spawn(move || server.serve_one());

    let mut conn = RdpConnection::connect("127.0.0.1", port, TIMEOUT).unwrap();
    let cookies = StorageActor::list_cookies(conn.transport_mut(), &tab_actor.into()).unwrap();

    assert_eq!(cookies.len(), 1);
    assert_eq!(cookies[0].name, "test_cookie");

    drop(conn);
    let requests = server_thread.join().unwrap();

    // Find the getStoreObjects request and verify it includes options.sessionString.
    let get_store_req = requests
        .iter()
        .find(|r| r.get("type").and_then(|v| v.as_str()) == Some("getStoreObjects"))
        .expect("expected a getStoreObjects request");

    assert_eq!(
        get_store_req["options"]["sessionString"], "Session",
        "getStoreObjects must include options.sessionString to avoid Firefox 149 crash"
    );
    assert_eq!(get_store_req["host"], "https://example.com");
    assert_eq!(get_store_req["resourceId"], "cookies-12345");
}
