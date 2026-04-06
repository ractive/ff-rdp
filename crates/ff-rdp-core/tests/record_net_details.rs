#![allow(clippy::collapsible_if, unused_variables)]
/// Record NetworkEventActor detail fixtures.
/// Run: cargo test -p ff-rdp-core --test record_net_details -- --ignored --nocapture
use std::time::Duration;

use ff_rdp_core::{RdpConnection, RdpTransport};
use serde_json::{Value, json};

fn connect() -> RdpConnection {
    RdpConnection::connect("127.0.0.1", 6000, Duration::from_secs(5)).expect("connect")
}

fn print_fixture(name: &str, value: &Value) {
    println!("=== {name} ===");
    println!("{}", serde_json::to_string_pretty(value).unwrap());
    println!();
}

fn recv_skip_events(transport: &mut RdpTransport, expected_from: &str) -> Value {
    loop {
        let msg = transport.recv().expect("recv");
        let from = msg.get("from").and_then(Value::as_str).unwrap_or_default();
        if from == expected_from {
            let msg_type = msg.get("type").and_then(Value::as_str).unwrap_or_default();
            if matches!(
                msg_type,
                "frameUpdate"
                    | "tabNavigated"
                    | "tabListChanged"
                    | "resources-available-array"
                    | "resources-updated-array"
            ) {
                continue;
            }
            return msg;
        }
    }
}

fn drain(transport: &mut RdpTransport, timeout: Duration) -> Vec<Value> {
    let mut msgs = Vec::new();
    let start = std::time::Instant::now();
    loop {
        if start.elapsed() > timeout {
            break;
        }
        match transport.recv() {
            Ok(msg) => msgs.push(msg),
            Err(_) => break,
        }
    }
    msgs
}

#[test]
#[ignore = "requires live Firefox on port 6000"]
fn record_network_event_actor_details() {
    let mut conn = connect();
    let t = conn.transport_mut();

    // Setup
    t.send(&json!({"to": "root", "type": "listTabs"})).unwrap();
    let tabs = recv_skip_events(t, "root");
    let tab_actor = tabs["tabs"][0]["actor"].as_str().unwrap().to_owned();

    t.send(&json!({"to": tab_actor, "type": "getTarget"}))
        .unwrap();
    let target = recv_skip_events(t, &tab_actor);
    let target_actor = target["frame"]["actor"].as_str().unwrap().to_owned();

    t.send(&json!({"to": tab_actor, "type": "getWatcher"}))
        .unwrap();
    let watcher = recv_skip_events(t, &tab_actor);
    let watcher_actor = watcher["actor"].as_str().unwrap().to_owned();
    println!("# Watcher: {watcher_actor}");

    // Watch network events
    t.send(&json!({
        "to": watcher_actor,
        "type": "watchResources",
        "resourceTypes": ["network-event"]
    }))
    .unwrap();
    drain(t, Duration::from_secs(2));

    // Navigate to trigger network activity
    t.send(&json!({
        "to": target_actor,
        "type": "navigateTo",
        "url": "https://example.com/"
    }))
    .unwrap();

    std::thread::sleep(Duration::from_secs(3));
    let events = drain(t, Duration::from_secs(3));

    // Find network event actor
    let mut net_actor = None;
    for msg in &events {
        let msg_type = msg.get("type").and_then(Value::as_str).unwrap_or_default();
        if msg_type == "resources-available-array" {
            if let Some(array) = msg.get("array").and_then(Value::as_array) {
                for sub in array {
                    if let Some(sub_arr) = sub.as_array() {
                        // sub_arr = ["network-event", [{actor: ...}, ...]]
                        if sub_arr.len() == 2 {
                            if let Some(resources) = sub_arr[1].as_array() {
                                for res in resources {
                                    if let Some(actor) = res.get("actor").and_then(Value::as_str) {
                                        if actor.contains("netEvent") {
                                            println!("# Found: {actor}");
                                            // Print the full resource-available-array for fixture
                                            print_fixture("resources_available_array", msg);
                                            net_actor = Some(actor.to_owned());
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        if msg_type == "resources-updated-array" {
            print_fixture("resources_updated_array", msg);
        }
    }

    let net_actor = net_actor.expect("should find a netEvent actor");

    // Fetch details — need a fresh connection since timeout ate our socket
    drop(conn);
    let mut conn2 = connect();
    let t2 = conn2.transport_mut();

    // Re-setup
    t2.send(&json!({"to": "root", "type": "listTabs"})).unwrap();
    let tabs2 = recv_skip_events(t2, "root");
    let tab_actor2 = tabs2["tabs"][0]["actor"].as_str().unwrap().to_owned();

    t2.send(&json!({"to": tab_actor2, "type": "getTarget"}))
        .unwrap();
    let target2 = recv_skip_events(t2, &tab_actor2);
    let target_actor2 = target2["frame"]["actor"].as_str().unwrap().to_owned();

    t2.send(&json!({"to": tab_actor2, "type": "getWatcher"}))
        .unwrap();
    let watcher2 = recv_skip_events(t2, &tab_actor2);
    let watcher_actor2 = watcher2["actor"].as_str().unwrap().to_owned();

    // Watch and navigate again to get fresh network events
    t2.send(&json!({
        "to": watcher_actor2,
        "type": "watchResources",
        "resourceTypes": ["network-event"]
    }))
    .unwrap();
    drain(t2, Duration::from_secs(2));

    t2.send(&json!({
        "to": target_actor2,
        "type": "navigateTo",
        "url": "https://example.com/"
    }))
    .unwrap();
    std::thread::sleep(Duration::from_secs(3));
    let events2 = drain(t2, Duration::from_secs(3));

    // Find fresh network event actor
    let mut fresh_net_actor = None;
    for msg in &events2 {
        let msg_type = msg.get("type").and_then(Value::as_str).unwrap_or_default();
        if msg_type == "resources-available-array" {
            if let Some(array) = msg.get("array").and_then(Value::as_array) {
                for sub in array {
                    if let Some(sub_arr) = sub.as_array() {
                        if sub_arr.len() == 2 {
                            if let Some(resources) = sub_arr[1].as_array() {
                                for res in resources {
                                    if let Some(actor) = res.get("actor").and_then(Value::as_str) {
                                        if actor.contains("netEvent") && fresh_net_actor.is_none() {
                                            // Only take the first (main document request)
                                            if res
                                                .get("cause")
                                                .and_then(|c| c.get("type"))
                                                .and_then(Value::as_str)
                                                == Some("document")
                                            {
                                                println!("# Fresh net actor: {actor}");
                                                fresh_net_actor = Some(actor.to_owned());
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let fresh_net_actor = fresh_net_actor.expect("should find fresh netEvent actor");
    println!("# Querying details for: {fresh_net_actor}\n");

    // getRequestHeaders
    t2.send(&json!({"to": fresh_net_actor, "type": "getRequestHeaders"}))
        .unwrap();
    let resp = recv_skip_events(t2, &fresh_net_actor);
    print_fixture("get_request_headers_response", &resp);

    // getResponseHeaders
    t2.send(&json!({"to": fresh_net_actor, "type": "getResponseHeaders"}))
        .unwrap();
    let resp = recv_skip_events(t2, &fresh_net_actor);
    print_fixture("get_response_headers_response", &resp);

    // getResponseContent
    t2.send(&json!({"to": fresh_net_actor, "type": "getResponseContent"}))
        .unwrap();
    let resp = recv_skip_events(t2, &fresh_net_actor);
    print_fixture("get_response_content_response", &resp);

    // getEventTimings
    t2.send(&json!({"to": fresh_net_actor, "type": "getEventTimings"}))
        .unwrap();
    let resp = recv_skip_events(t2, &fresh_net_actor);
    print_fixture("get_event_timings_response", &resp);

    println!("# Done!");
}
