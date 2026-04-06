/// Probe the Firefox StorageActor protocol to discover method names and response formats.
///
/// Run with: cargo test -p ff-rdp-core --test probe_storage_actor -- --ignored --nocapture
mod support;

use std::time::Duration;

use ff_rdp_core::{RdpConnection, RdpTransport, RootActor, TabActor};
use serde_json::{Value, json};

fn connect() -> RdpConnection {
    RdpConnection::connect("localhost", 6000, Duration::from_secs(10)).expect("connect to Firefox")
}

fn get_target(t: &mut RdpTransport) -> (String, Value) {
    let tabs = RootActor::list_tabs(t).unwrap();
    let tab = &tabs[0];
    println!("\nTab: {} - {}", tab.title, tab.url);

    // Raw getTarget to see ALL fields
    t.send(&json!({"to": tab.actor.as_ref(), "type": "getTarget"}))
        .unwrap();
    let resp = loop {
        let msg = t.recv().unwrap();
        if msg.get("frame").is_some() {
            break msg;
        }
    };
    let frame = resp["frame"].clone();
    (tab.actor.as_ref().to_owned(), frame)
}

fn drain(t: &mut RdpTransport) -> Vec<Value> {
    let mut msgs = Vec::new();
    let start = std::time::Instant::now();
    loop {
        if start.elapsed() > Duration::from_millis(500) {
            break;
        }
        match t.recv() {
            Ok(msg) => msgs.push(msg),
            Err(_) => break,
        }
    }
    msgs
}

#[test]
#[ignore = "requires a live Firefox instance"]
fn probe_target_actors() {
    let mut conn = connect();
    let t = conn.transport_mut();
    let (_tab_actor, frame) = get_target(t);

    println!("\n=== All fields in frame ===");
    if let Some(obj) = frame.as_object() {
        for (key, val) in obj {
            if key.to_lowercase().contains("actor") || key.to_lowercase().contains("storage") {
                println!("  {key}: {val}");
            }
        }
    }

    // Also print ALL keys for completeness
    println!("\n=== All frame keys ===");
    if let Some(obj) = frame.as_object() {
        for key in obj.keys() {
            println!("  {key}");
        }
    }
}

#[test]
#[ignore = "requires a live Firefox instance"]
fn probe_watcher_resource_types() {
    // The StorageActor might be accessed via WatcherActor resources
    let mut conn = connect();
    let t = conn.transport_mut();
    let tabs = RootActor::list_tabs(t).unwrap();
    let _tab_actor = tabs[0].actor.as_ref().to_owned();

    // Get watcher
    let watcher = TabActor::get_watcher(t, &tabs[0].actor).unwrap();
    println!("\nWatcher actor: {watcher}");

    // Try watching for storage-related resources
    // Known resource types from Firefox source: "cookie", "local-storage", "session-storage", etc.
    for resource_type in &[
        "cookie",
        "cookies",
        "storage",
        "local-storage",
        "session-storage",
        "indexed-db",
        "cache-storage",
    ] {
        println!("\n--- Trying watchResources({resource_type}) ---");
        t.send(&json!({
            "to": watcher.as_ref(),
            "type": "watchResources",
            "resourceTypes": [resource_type]
        }))
        .unwrap();

        // Read response
        let resp = loop {
            let msg = t.recv().unwrap();
            let from = msg.get("from").and_then(Value::as_str).unwrap_or_default();
            if from == watcher.as_ref() {
                break msg;
            }
            // Also print what we skip
            let skip_type = msg.get("type").and_then(Value::as_str).unwrap_or("?");
            println!("  (skipped: type={skip_type}, from={from})");
        };

        if resp.get("error").is_some() {
            println!("  ERROR: {}", serde_json::to_string_pretty(&resp).unwrap());
        } else {
            println!("  OK: {}", serde_json::to_string_pretty(&resp).unwrap());
        }

        // Drain any follow-up messages
        let extras = drain(t);
        for msg in &extras {
            let msg_type = msg.get("type").and_then(Value::as_str).unwrap_or("?");
            println!(
                "  Follow-up (type={msg_type}): {}",
                serde_json::to_string(&msg)
                    .unwrap()
                    .chars()
                    .take(300)
                    .collect::<String>()
            );
        }
    }
}

#[test]
#[ignore = "requires a live Firefox instance"]
fn probe_storage_via_console_actor() {
    // Try accessing storage through the console's evaluateJSAsync
    // to see if there are storageActor methods
    let mut conn = connect();
    let t = conn.transport_mut();
    let (_tab_actor, frame) = get_target(t);

    // First, set a test cookie via JS eval so we have something to find
    let console_actor = frame["consoleActor"].as_str().unwrap().to_owned();
    t.send(&json!({
        "to": &console_actor,
        "type": "evaluateJSAsync",
        "text": "document.cookie = 'testcookie=hello; path=/'",
        "eager": false
    }))
    .unwrap();
    let _ = drain(t);

    // Now look for storageActor in the frame
    // Try various possible actor names
    for key in &["storageActor", "Storage", "storage"] {
        if let Some(actor) = frame.get(*key).and_then(Value::as_str) {
            println!("Found storage actor at frame.{key}: {actor}");

            // Try listStores
            t.send(&json!({"to": actor, "type": "listStores"})).unwrap();
            let resp = t.recv().unwrap();
            println!(
                "listStores response: {}",
                serde_json::to_string_pretty(&resp).unwrap()
            );
        }
    }

    // Maybe storage is accessed through a resource on the target actor
    let target_actor = frame["actor"].as_str().unwrap();
    println!("\n--- Trying getStorageActor on target ---");
    t.send(&json!({"to": target_actor, "type": "getStorageActor"}))
        .unwrap();
    let resp = t.recv().unwrap();
    println!("Response: {}", serde_json::to_string_pretty(&resp).unwrap());

    // Maybe it's through the Watcher
    println!("\n--- Trying to access storage via traits ---");
    t.send(&json!({"to": "root", "type": "listTabs"})).unwrap();
    let resp = loop {
        let msg = t.recv().unwrap();
        if msg.get("tabs").is_some() {
            break msg;
        }
    };
    // Check if root traits mention storage
    if let Some(traits) = resp.get("traits") {
        println!(
            "Root traits: {}",
            serde_json::to_string_pretty(traits).unwrap()
        );
    }
}

#[test]
#[ignore = "requires a live Firefox instance"]
fn probe_cookies_actor_methods() {
    // First set a cookie, then discover how to read it via the cookies actor
    let mut conn = connect();
    let t = conn.transport_mut();
    let (_tab_actor, frame) = get_target(t);

    // Set test cookies via JS — one with expiry to avoid Firefox sort bug on session cookies
    let console_actor = frame["consoleActor"].as_str().unwrap().to_owned();
    t.send(&json!({
        "to": &console_actor,
        "type": "evaluateJSAsync",
        "text": "document.cookie = 'probecookie=discovery123; path=/; expires=Fri, 31 Dec 2027 23:59:59 GMT'; document.cookie = 'sessioncookie=abc; path=/'",
        "eager": false
    }))
    .unwrap();
    let _ = drain(t);

    // Get the cookies actor via watchResources
    let tabs = RootActor::list_tabs(t).unwrap();
    let watcher = TabActor::get_watcher(t, &tabs[0].actor).unwrap();

    t.send(&json!({
        "to": watcher.as_ref(),
        "type": "watchResources",
        "resourceTypes": ["cookies"]
    }))
    .unwrap();

    // Find the cookies actor from the response
    let mut cookies_actor = String::new();
    let mut host = String::new();
    loop {
        let msg = t.recv().unwrap();
        if let Some(array) = msg.get("array").and_then(Value::as_array) {
            for entry in array {
                if let Some(resources) = entry
                    .as_array()
                    .and_then(|a| a.get(1))
                    .and_then(Value::as_array)
                {
                    for resource in resources {
                        if let Some(actor) = resource.get("actor").and_then(Value::as_str) {
                            cookies_actor = actor.to_owned();
                        }
                        if let Some(hosts) = resource.get("hosts").and_then(Value::as_object)
                            && let Some(h) = hosts.keys().next()
                        {
                            host = h.clone();
                        }
                    }
                }
            }
            break;
        }
    }
    let _ = drain(t);
    println!("\nCookies actor: {cookies_actor}");
    println!("Host: {host}");

    // Now try various methods on the cookies actor
    let methods = [
        (
            "getStoreObjects",
            json!({"to": &cookies_actor, "type": "getStoreObjects", "host": &host}),
        ),
        (
            "getFields",
            json!({"to": &cookies_actor, "type": "getFields"}),
        ),
        (
            "getEditableFields",
            json!({"to": &cookies_actor, "type": "getEditableFields"}),
        ),
    ];

    for (name, request) in &methods {
        println!("\n--- {name} ---");
        t.send(request).unwrap();

        let start = std::time::Instant::now();
        loop {
            if start.elapsed() > Duration::from_secs(3) {
                println!("  (timed out)");
                break;
            }
            match t.recv() {
                Ok(msg) => {
                    let from = msg.get("from").and_then(Value::as_str).unwrap_or_default();
                    if from == cookies_actor {
                        println!(
                            "  Response: {}",
                            serde_json::to_string_pretty(&msg).unwrap()
                        );
                        break;
                    }
                    println!(
                        "  (other: from={from}, type={})",
                        msg.get("type").and_then(Value::as_str).unwrap_or("?")
                    );
                }
                Err(_) => break,
            }
        }
        let _ = drain(t);
    }

    // Try getStoreObjects with various option combos
    let variations = [
        (
            "with options object",
            json!({
                "to": &cookies_actor,
                "type": "getStoreObjects",
                "host": &host,
                "options": {}
            }),
        ),
        (
            "with sortOn",
            json!({
                "to": &cookies_actor,
                "type": "getStoreObjects",
                "host": &host,
                "options": {"sortOn": "name", "sortBy": "name"}
            }),
        ),
        (
            "with name filter array",
            json!({
                "to": &cookies_actor,
                "type": "getStoreObjects",
                "host": &host,
                "names": ["probecookie"]
            }),
        ),
        (
            "no host",
            json!({
                "to": &cookies_actor,
                "type": "getStoreObjects"
            }),
        ),
    ];

    // Try with uniqueKey format (from getFields: uniqueKey is a private field)
    let more_variations = [
        (
            "with name filter probecookie",
            json!({
                "to": &cookies_actor,
                "type": "getStoreObjects",
                "host": &host,
                "names": ["probecookie"]
            }),
        ),
        (
            "with name filter sessioncookie",
            json!({
                "to": &cookies_actor,
                "type": "getStoreObjects",
                "host": &host,
                "names": ["sessioncookie"]
            }),
        ),
        (
            "with empty names array",
            json!({
                "to": &cookies_actor,
                "type": "getStoreObjects",
                "host": &host,
                "names": []
            }),
        ),
    ];

    // Try reading the uniqueKey format via JS eval
    println!("\n--- Reading Firefox cookie storage source ---");
    t.send(&json!({
        "to": &console_actor,
        "type": "evaluateJSAsync",
        "text": "JSON.stringify(document.cookie)",
        "eager": false
    }))
    .unwrap();
    let start = std::time::Instant::now();
    loop {
        if start.elapsed() > Duration::from_secs(3) {
            break;
        }
        match t.recv() {
            Ok(msg) => {
                if msg.get("result").is_some() || msg.get("resultID").is_some() {
                    println!(
                        "  Cookies via JS: {}",
                        serde_json::to_string_pretty(&msg).unwrap()
                    );
                    break;
                }
            }
            Err(_) => break,
        }
    }
    let _ = drain(t);

    // Firefox uniqueKey = name + SEPARATOR_GUID + host + SEPARATOR_GUID + path + SEPARATOR_GUID + partitionKey
    let sep = "{9d414cc5-8319-0a04-0586-c0a6ae01670a}";
    let key_formats = [
        format!("probecookie{sep}example.com{sep}/{sep}"),
        format!("probecookie{sep}.example.com{sep}/{sep}"),
    ];
    for key in &key_formats {
        println!("\n--- getStoreObjects with uniqueKey: {key} ---");
        t.send(&json!({
            "to": &cookies_actor,
            "type": "getStoreObjects",
            "host": &host,
            "names": [key]
        }))
        .unwrap();
        let start = std::time::Instant::now();
        loop {
            if start.elapsed() > Duration::from_secs(3) {
                println!("  (timed out)");
                break;
            }
            match t.recv() {
                Ok(msg) => {
                    let from = msg.get("from").and_then(Value::as_str).unwrap_or_default();
                    if from == cookies_actor {
                        let data_str = serde_json::to_string(&msg["data"]).unwrap();
                        if data_str == "[null]" {
                            println!("  null");
                        } else {
                            println!("  FOUND: {}", serde_json::to_string_pretty(&msg).unwrap());
                        }
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        let _ = drain(t);
    }

    // Try getStoreObjects with sessionString in options to fix the sort bug
    println!("\n--- getStoreObjects with sessionString option ---");
    t.send(&json!({
        "to": &cookies_actor,
        "type": "getStoreObjects",
        "host": &host,
        "options": {"sessionString": "Session", "sortOn": "name"}
    }))
    .unwrap();
    let start = std::time::Instant::now();
    loop {
        if start.elapsed() > Duration::from_secs(5) {
            println!("  (timed out)");
            break;
        }
        match t.recv() {
            Ok(msg) => {
                let from = msg.get("from").and_then(Value::as_str).unwrap_or_default();
                if from == cookies_actor {
                    println!(
                        "  Response: {}",
                        serde_json::to_string_pretty(&msg).unwrap()
                    );
                    break;
                }
            }
            Err(_) => break,
        }
    }
    let _ = drain(t);

    for (label, request) in &more_variations {
        println!("\n--- getStoreObjects {label} ---");
        t.send(request).unwrap();
        let start = std::time::Instant::now();
        loop {
            if start.elapsed() > Duration::from_secs(3) {
                println!("  (timed out)");
                break;
            }
            match t.recv() {
                Ok(msg) => {
                    let from = msg.get("from").and_then(Value::as_str).unwrap_or_default();
                    if from == cookies_actor {
                        println!(
                            "  Response: {}",
                            serde_json::to_string_pretty(&msg).unwrap()
                        );
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        let _ = drain(t);
    }

    for (label, request) in &variations {
        println!("\n--- getStoreObjects {label} ---");
        t.send(request).unwrap();
        let start = std::time::Instant::now();
        loop {
            if start.elapsed() > Duration::from_secs(3) {
                println!("  (timed out)");
                break;
            }
            match t.recv() {
                Ok(msg) => {
                    let from = msg.get("from").and_then(Value::as_str).unwrap_or_default();
                    if from == cookies_actor {
                        println!(
                            "  Response: {}",
                            serde_json::to_string_pretty(&msg).unwrap()
                        );
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        let _ = drain(t);
    }
}
