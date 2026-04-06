#![allow(clippy::collapsible_if)]
/// Record fixtures for iteration 4 (console + network monitoring).
///
/// Run with:
///   cargo test -p ff-rdp-core --test record_iter4_fixtures -- --ignored --nocapture
///
/// Requires Firefox running with:
///   /Applications/Firefox.app/Contents/MacOS/firefox -no-remote \
///     -profile /tmp/ff-rdp-test-profile --start-debugger-server 6000 --headless
///
/// Navigate to example.com first:
///   cargo run -- --host 127.0.0.1 --port 6000 navigate https://example.com
///
/// Generate console messages:
///   cargo run -- --host 127.0.0.1 --port 6000 eval "console.log('hello'); console.warn('warn msg'); console.error('error msg'); 'done'"
use std::time::Duration;

use ff_rdp_core::{RdpConnection, RdpTransport};
use serde_json::{Value, json};

fn connect() -> RdpConnection {
    RdpConnection::connect("127.0.0.1", 6000, Duration::from_secs(5)).expect("connect to Firefox")
}

fn print_fixture(name: &str, value: &Value) {
    println!("=== {name} ===");
    println!("{}", serde_json::to_string_pretty(value).unwrap());
    println!();
}

/// Read messages until we get a response from the expected actor,
/// discarding unsolicited events.
fn recv_from_actor(transport: &mut RdpTransport, expected_from: &str) -> Value {
    loop {
        let msg = transport.recv().expect("recv");
        let from = msg.get("from").and_then(Value::as_str).unwrap_or_default();
        if from == expected_from {
            let msg_type = msg.get("type").and_then(Value::as_str).unwrap_or_default();
            // Skip known async events
            if msg_type == "frameUpdate"
                || msg_type == "tabNavigated"
                || msg_type == "tabListChanged"
            {
                print_fixture(&format!("(event: {msg_type})"), &msg);
                continue;
            }
            return msg;
        }
        let msg_type = msg.get("type").and_then(Value::as_str).unwrap_or_default();
        print_fixture(&format!("(discarded: from={from} type={msg_type})"), &msg);
    }
}

/// Read all messages with a short timeout, collecting them.
fn drain_messages(transport: &mut RdpTransport, timeout: Duration) -> Vec<Value> {
    // Temporarily set a short read timeout to drain events
    let mut messages = Vec::new();
    let start = std::time::Instant::now();
    loop {
        if start.elapsed() > timeout {
            break;
        }
        match transport.recv() {
            Ok(msg) => messages.push(msg),
            Err(_) => break,
        }
    }
    messages
}

#[test]
#[ignore = "requires a live Firefox instance on port 6000"]
fn record_console_fixtures() {
    let mut conn = connect();
    let transport = conn.transport_mut();

    // 1. listTabs
    transport
        .send(&json!({"to": "root", "type": "listTabs"}))
        .expect("send");
    let list_tabs_resp = recv_from_actor(transport, "root");
    print_fixture("list_tabs_response", &list_tabs_resp);

    let tab_actor = list_tabs_resp["tabs"][0]["actor"]
        .as_str()
        .expect("tab actor")
        .to_owned();

    // 2. getTarget
    transport
        .send(&json!({"to": tab_actor, "type": "getTarget"}))
        .expect("send");
    let get_target_resp = recv_from_actor(transport, &tab_actor);
    print_fixture("get_target_response", &get_target_resp);

    let console_actor = get_target_resp["frame"]["consoleActor"]
        .as_str()
        .expect("console actor")
        .to_owned();
    println!("# Console actor: {console_actor}\n");

    // 3. startListeners
    transport
        .send(&json!({
            "to": console_actor,
            "type": "startListeners",
            "listeners": ["PageError", "ConsoleAPI"]
        }))
        .expect("send");
    let start_listeners_resp = recv_from_actor(transport, &console_actor);
    print_fixture("start_listeners_response", &start_listeners_resp);

    // 4. getCachedMessages
    transport
        .send(&json!({
            "to": console_actor,
            "type": "getCachedMessages",
            "messageTypes": ["PageError", "ConsoleAPI"]
        }))
        .expect("send");
    let cached_msgs_resp = recv_from_actor(transport, &console_actor);
    print_fixture("get_cached_messages_response", &cached_msgs_resp);

    // Count messages
    if let Some(messages) = cached_msgs_resp.get("messages").and_then(Value::as_array) {
        println!("# Found {} cached messages", messages.len());
        for (i, msg) in messages.iter().enumerate() {
            let msg_type = msg.get("_type").and_then(Value::as_str).unwrap_or("?");
            let level = msg.get("level").and_then(Value::as_str).unwrap_or("?");
            println!("  [{i}] type={msg_type} level={level}");
        }
    }

    println!("\n# Done recording console fixtures!");
}

#[test]
#[ignore = "requires a live Firefox instance on port 6000"]
fn record_network_fixtures() {
    let mut conn = connect();
    let transport = conn.transport_mut();

    // 1. listTabs + getTarget
    transport
        .send(&json!({"to": "root", "type": "listTabs"}))
        .expect("send");
    let list_tabs_resp = recv_from_actor(transport, "root");

    let tab_actor = list_tabs_resp["tabs"][0]["actor"]
        .as_str()
        .expect("tab actor")
        .to_owned();

    transport
        .send(&json!({"to": tab_actor, "type": "getTarget"}))
        .expect("send");
    let _get_target_resp = recv_from_actor(transport, &tab_actor);

    // 2. getWatcher
    transport
        .send(&json!({"to": tab_actor, "type": "getWatcher"}))
        .expect("send");
    let watcher_resp = recv_from_actor(transport, &tab_actor);
    print_fixture("get_watcher_response", &watcher_resp);

    let watcher_actor = watcher_resp["actor"]
        .as_str()
        .expect("watcher actor")
        .to_owned();
    println!("# Watcher actor: {watcher_actor}\n");

    // 3. watchResources for network events
    transport
        .send(&json!({
            "to": watcher_actor,
            "type": "watchResources",
            "resourceTypes": ["network-event"]
        }))
        .expect("send");

    // Read all messages that come back (response + resource-available-form events)
    println!("# Reading watchResources response + events...\n");
    let messages = drain_messages(transport, Duration::from_secs(3));
    for (i, msg) in messages.iter().enumerate() {
        let msg_type = msg.get("type").and_then(Value::as_str).unwrap_or("?");
        let from = msg.get("from").and_then(Value::as_str).unwrap_or("?");
        print_fixture(
            &format!("watchResources msg [{i}] from={from} type={msg_type}"),
            msg,
        );
    }

    // 4. Now navigate to trigger network activity, then collect events
    drop(conn);
    println!("\n# --- Reconnecting for network capture during navigation ---\n");

    let mut conn2 = connect();
    let transport2 = conn2.transport_mut();

    // Setup
    transport2
        .send(&json!({"to": "root", "type": "listTabs"}))
        .expect("send");
    let list_tabs2 = recv_from_actor(transport2, "root");
    let tab_actor2 = list_tabs2["tabs"][0]["actor"]
        .as_str()
        .expect("tab actor")
        .to_owned();

    transport2
        .send(&json!({"to": tab_actor2, "type": "getTarget"}))
        .expect("send");
    let target_resp2 = recv_from_actor(transport2, &tab_actor2);
    let target_actor2 = target_resp2["frame"]["actor"]
        .as_str()
        .expect("target actor")
        .to_owned();

    // Get watcher
    transport2
        .send(&json!({"to": tab_actor2, "type": "getWatcher"}))
        .expect("send");
    let watcher_resp2 = recv_from_actor(transport2, &tab_actor2);
    let watcher_actor2 = watcher_resp2["actor"]
        .as_str()
        .expect("watcher actor")
        .to_owned();

    // Watch network events BEFORE navigation
    transport2
        .send(&json!({
            "to": watcher_actor2,
            "type": "watchResources",
            "resourceTypes": ["network-event"]
        }))
        .expect("send");

    // Drain initial events
    let initial_events = drain_messages(transport2, Duration::from_secs(2));
    println!(
        "# Drained {} initial events after watchResources",
        initial_events.len()
    );
    for (i, msg) in initial_events.iter().enumerate() {
        let msg_type = msg.get("type").and_then(Value::as_str).unwrap_or("?");
        print_fixture(&format!("initial event [{i}] type={msg_type}"), msg);
    }

    // Navigate to trigger network activity
    transport2
        .send(&json!({
            "to": target_actor2,
            "type": "navigateTo",
            "url": "https://example.com/"
        }))
        .expect("send");

    // Wait for navigation and collect all network events
    std::thread::sleep(Duration::from_secs(3));
    let nav_events = drain_messages(transport2, Duration::from_secs(3));
    println!("\n# Collected {} events after navigation", nav_events.len());

    let mut network_event_actor = None;
    for (i, msg) in nav_events.iter().enumerate() {
        let msg_type = msg.get("type").and_then(Value::as_str).unwrap_or("?");
        let from = msg.get("from").and_then(Value::as_str).unwrap_or("?");

        if msg_type == "resource-available-form" {
            print_fixture(&format!("network event [{i}] type={msg_type}"), msg);

            // Extract network event actor IDs from resources
            if let Some(array) = msg.get("array").and_then(Value::as_array) {
                for sub in array {
                    if let Some(sub_arr) = sub.as_array() {
                        for item in sub_arr {
                            if let Some(actor) = item.get("actor").and_then(Value::as_str) {
                                if actor.contains("netEvent") {
                                    println!("  # Found network event actor: {actor}");
                                    network_event_actor = Some(actor.to_owned());
                                }
                            }
                        }
                    }
                }
            }
            // Also check "resources" field
            if let Some(resources) = msg.get("resources").and_then(Value::as_array) {
                for res in resources {
                    if let Some(actor) = res.get("actor").and_then(Value::as_str) {
                        if actor.contains("netEvent") {
                            println!("  # Found network event actor: {actor}");
                            network_event_actor = Some(actor.to_owned());
                        }
                    }
                }
            }
        } else if msg_type == "resource-updated-form" {
            print_fixture(&format!("network update [{i}] type={msg_type}"), msg);
        } else {
            print_fixture(
                &format!("other event [{i}] from={from} type={msg_type}"),
                msg,
            );
        }
    }

    // 5. If we got a network event actor, fetch its details
    if let Some(net_actor) = network_event_actor {
        println!("\n# --- Fetching NetworkEventActor details for {net_actor} ---\n");

        // getRequestHeaders
        transport2
            .send(&json!({"to": net_actor, "type": "getRequestHeaders"}))
            .expect("send");
        let req_headers = recv_from_actor(transport2, &net_actor);
        print_fixture("get_request_headers_response", &req_headers);

        // getResponseHeaders
        transport2
            .send(&json!({"to": net_actor, "type": "getResponseHeaders"}))
            .expect("send");
        let resp_headers = recv_from_actor(transport2, &net_actor);
        print_fixture("get_response_headers_response", &resp_headers);

        // getResponseContent
        transport2
            .send(&json!({"to": net_actor, "type": "getResponseContent"}))
            .expect("send");
        let resp_content = recv_from_actor(transport2, &net_actor);
        print_fixture("get_response_content_response", &resp_content);

        // getEventTimings
        transport2
            .send(&json!({"to": net_actor, "type": "getEventTimings"}))
            .expect("send");
        let event_timings = recv_from_actor(transport2, &net_actor);
        print_fixture("get_event_timings_response", &event_timings);
    } else {
        println!("\n# WARNING: No network event actor found! Check the events above.");
    }

    // 6. unwatchResources
    transport2
        .send(&json!({
            "to": watcher_actor2,
            "type": "unwatchResources",
            "resourceTypes": ["network-event"]
        }))
        .expect("send");
    let unwatch_resp = recv_from_actor(transport2, &watcher_actor2);
    print_fixture("unwatch_resources_response", &unwatch_resp);

    println!("\n# Done recording network fixtures!");
}
