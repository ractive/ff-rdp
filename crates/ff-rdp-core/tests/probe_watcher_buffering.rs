/// Probe whether WatcherActor replays buffered network events on a fresh `watchResources`
/// subscription when the page already has network activity.
///
/// Run with:
///   cargo test -p ff-rdp-core --test probe_watcher_buffering -- --ignored --nocapture
///
/// Requires Firefox running with:
///   /Applications/Firefox.app/Contents/MacOS/firefox -no-remote \
///     -profile /tmp/ff-rdp-test-profile --start-debugger-server 6000 --headless
///
/// Navigate to a page that generates network activity first, e.g.:
///   cargo run -- --host 127.0.0.1 --port 6000 navigate https://example.com
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use ff_rdp_core::{RdpConnection, RdpTransport};
use serde_json::{Value, json};

fn connect() -> RdpConnection {
    RdpConnection::connect("127.0.0.1", 6000, Duration::from_secs(5)).expect("connect to Firefox")
}

/// Read messages until we receive a reply from `expected_from`, discarding
/// unsolicited events from other actors.
fn recv_from_actor(transport: &mut RdpTransport, expected_from: &str) -> Value {
    loop {
        let msg = transport.recv().expect("recv");
        let from = msg.get("from").and_then(Value::as_str).unwrap_or_default();
        if from == expected_from {
            let msg_type = msg.get("type").and_then(Value::as_str).unwrap_or_default();
            // Skip known async lifecycle events
            if matches!(msg_type, "frameUpdate" | "tabNavigated" | "tabListChanged") {
                println!("(skipped async event from {from}: type={msg_type})");
                continue;
            }
            return msg;
        }
        let msg_type = msg.get("type").and_then(Value::as_str).unwrap_or_default();
        println!("(discarded: from={from} type={msg_type})");
    }
}

/// Drain all messages that arrive within `timeout`, returning them in order.
/// Stops as soon as a recv call fails (timeout or EOF).
fn drain_with_timeout(transport: &mut RdpTransport, timeout: Duration) -> Vec<Value> {
    let mut messages = Vec::new();
    let deadline = Instant::now() + timeout;
    loop {
        if Instant::now() >= deadline {
            break;
        }
        match transport.recv() {
            Ok(msg) => messages.push(msg),
            Err(_) => break,
        }
    }
    messages
}

/// Print a compact summary line for a message: its `type`, `from`, and the
/// top-level keys present.  For `resources-available-array` also prints how
/// many resources arrived and their `resourceType` values.
fn summarise(index: usize, msg: &Value) {
    let msg_type = msg.get("type").and_then(Value::as_str).unwrap_or("?");
    let from = msg.get("from").and_then(Value::as_str).unwrap_or("?");

    let keys: Vec<&str> = msg
        .as_object()
        .map(|o| o.keys().map(String::as_str).collect())
        .unwrap_or_default();

    println!("[{index}] type={msg_type} from={from}  keys={keys:?}");

    // Dig into resource payloads
    match msg_type {
        "resources-available-array" => {
            if let Some(array) = msg.get("array").and_then(Value::as_array) {
                println!(
                    "  resources-available-array: {} resource group(s)",
                    array.len()
                );
                for (gi, group) in array.iter().enumerate() {
                    if let Some(pair) = group.as_array() {
                        // Firefox often sends [resourceType, [resource, ...]]
                        let rtype = pair.first().and_then(Value::as_str).unwrap_or("?");
                        let count = pair.get(1).and_then(Value::as_array).map_or(0, Vec::len);
                        println!("    group[{gi}]: resourceType={rtype} count={count}");
                    } else {
                        println!("    group[{gi}]: (unexpected shape) {group}");
                    }
                }
            } else {
                println!("  (no `array` field — full message below)");
                println!(
                    "  {}",
                    serde_json::to_string_pretty(msg).unwrap_or_default()
                );
            }
        }
        "resource-available-form" => {
            // Older Firefox protocol shape
            println!("  resource-available-form payload:");
            println!(
                "  {}",
                serde_json::to_string_pretty(msg).unwrap_or_default()
            );
        }
        _ => {
            // For anything else print the full JSON so we don't miss surprises
            println!(
                "  {}",
                serde_json::to_string_pretty(msg).unwrap_or_default()
            );
        }
    }
}

#[test]
#[ignore = "requires a live Firefox instance on port 6000 with prior network activity"]
fn probe_watcher_buffering() {
    let mut conn = connect();
    let transport = conn.transport_mut();

    // ── Step 1: listTabs ──────────────────────────────────────────────────────
    transport
        .send(&json!({"to": "root", "type": "listTabs"}))
        .expect("send listTabs");
    let list_tabs = recv_from_actor(transport, "root");

    let tab_actor = list_tabs["tabs"][0]["actor"]
        .as_str()
        .expect("tab actor")
        .to_owned();
    println!("\n# tab actor: {tab_actor}");

    // ── Step 2: getTarget (confirms the tab is alive) ─────────────────────────
    transport
        .send(&json!({"to": tab_actor, "type": "getTarget"}))
        .expect("send getTarget");
    let get_target = recv_from_actor(transport, &tab_actor);
    let page_url = get_target
        .pointer("/frame/url")
        .and_then(Value::as_str)
        .unwrap_or("?");
    println!("# tab URL: {page_url}");

    // ── Step 3: getWatcher ────────────────────────────────────────────────────
    transport
        .send(&json!({"to": tab_actor, "type": "getWatcher"}))
        .expect("send getWatcher");
    let watcher_resp = recv_from_actor(transport, &tab_actor);

    let watcher_actor = watcher_resp["actor"]
        .as_str()
        .expect("watcher actor")
        .to_owned();
    println!("# watcher actor: {watcher_actor}\n");

    // ── Step 4: watchResources ────────────────────────────────────────────────
    println!("# Sending watchResources for [network-event]…");
    transport
        .send(&json!({
            "to": watcher_actor,
            "type": "watchResources",
            "resourceTypes": ["network-event"]
        }))
        .expect("send watchResources");

    // ── Step 5: Drain everything Firefox sends over the next 3 seconds ────────
    println!("# Draining messages for 3 seconds…\n");
    let messages = drain_with_timeout(transport, Duration::from_secs(3));

    println!("# Total messages received: {}\n", messages.len());

    if messages.is_empty() {
        println!("# !! Firefox sent NOTHING after watchResources — no buffered events.");
    } else {
        for (i, msg) in messages.iter().enumerate() {
            summarise(i, msg);
            println!();
        }

        // Quick tally
        let mut counts: std::collections::BTreeMap<&str, usize> = BTreeMap::default();
        for msg in &messages {
            let t = msg.get("type").and_then(Value::as_str).unwrap_or("?");
            *counts.entry(t).or_default() += 1;
        }
        println!("# Message type tally:");
        for (t, n) in &counts {
            println!("    {t}: {n}");
        }
    }

    println!("\n# Probe complete.");
}
