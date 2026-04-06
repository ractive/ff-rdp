//! Live tests that exercise real Firefox and optionally record fixtures.
//!
//! Run live tests only:
//!   FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-core --test live_record_fixtures -- --ignored
//!
//! Run live tests AND record fixtures:
//!   FF_RDP_LIVE_TESTS_RECORD=1 cargo test -p ff-rdp-core --test live_record_fixtures -- --ignored
//!
//! Requires Firefox with:
//!   firefox -no-remote -profile /tmp/ff-rdp-test-profile --start-debugger-server 6000 --headless

mod support;

use std::time::Duration;

use ff_rdp_core::{RdpConnection, RdpTransport};
use serde_json::{Value, json};
use support::recording::*;

const TIMEOUT: Duration = Duration::from_secs(10);

fn connect() -> RdpConnection {
    RdpConnection::connect("127.0.0.1", firefox_port(), TIMEOUT).expect("connect to Firefox")
}

// ===========================================================================
// Part B: Protocol-level fixtures
// ===========================================================================

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_handshake() {
    if !should_run_live() {
        return;
    }
    // The greeting message is consumed internally by RdpConnection::connect and is not
    // accessible after the fact. We simply verify that the connection succeeds.
    //
    // handshake.json is the one fixture maintained manually: the greeting format
    // ("from": "root", "applicationType": "browser", "traits": {...}) is stable and
    // well-known from the Firefox Remote Debugging Protocol spec.
    let conn = connect();
    drop(conn);
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_list_tabs() {
    if !should_run_live() {
        return;
    }
    let mut conn = connect();
    let transport = conn.transport_mut();

    let resp = send_raw(transport, &json!({"to": "root", "type": "listTabs"}));

    assert!(
        resp.get("tabs").and_then(Value::as_array).is_some(),
        "listTabs must return a tabs array"
    );

    save_cli_fixture("list_tabs_response.json", &resp);
    save_core_fixture("list_tabs_response.json", &resp);
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_get_target() {
    if !should_run_live() {
        return;
    }
    let mut conn = connect();
    let transport = conn.transport_mut();

    let list_tabs = send_raw(transport, &json!({"to": "root", "type": "listTabs"}));
    let tab_actor = list_tabs["tabs"][0]["actor"].as_str().expect("tab actor");

    let resp = send_raw(transport, &json!({"to": tab_actor, "type": "getTarget"}));

    assert!(resp.get("frame").is_some(), "getTarget must return a frame");
    assert!(
        resp["frame"].get("consoleActor").is_some(),
        "frame must have consoleActor"
    );

    save_cli_fixture("get_target_response.json", &resp);
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_get_watcher() {
    if !should_run_live() {
        return;
    }
    let mut conn = connect();
    let transport = conn.transport_mut();

    let list_tabs = send_raw(transport, &json!({"to": "root", "type": "listTabs"}));
    let tab_actor = list_tabs["tabs"][0]["actor"].as_str().expect("tab actor");

    let resp = send_raw(transport, &json!({"to": tab_actor, "type": "getWatcher"}));

    assert!(
        resp.get("actor").and_then(Value::as_str).is_some(),
        "getWatcher must return an actor"
    );

    save_cli_fixture("get_watcher_response.json", &resp);
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_watch_resources() {
    if !should_run_live() {
        return;
    }
    let mut conn = connect();
    let transport = conn.transport_mut();

    let list_tabs = send_raw(transport, &json!({"to": "root", "type": "listTabs"}));
    let tab_actor = list_tabs["tabs"][0]["actor"].as_str().expect("tab actor");

    let watcher_resp = send_raw(transport, &json!({"to": tab_actor, "type": "getWatcher"}));
    let watcher_actor = watcher_resp["actor"].as_str().expect("watcher actor");

    transport
        .send(&json!({
            "to": watcher_actor,
            "type": "watchResources",
            "resourceTypes": ["network-event"]
        }))
        .expect("send watchResources");

    let resp = recv_from_actor(transport, watcher_actor);

    assert!(
        resp.get("from").is_some(),
        "watchResources response must have a 'from' field"
    );
    save_cli_fixture("watch_resources_response.json", &resp);
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_navigate() {
    if !should_run_live() {
        return;
    }
    let mut conn = connect();
    let transport = conn.transport_mut();

    let (target_actor, _console_actor) = setup_target(transport);

    // Navigate to example.com
    transport
        .send(&json!({
            "to": &target_actor,
            "type": "navigateTo",
            "url": "https://example.com/"
        }))
        .expect("send navigateTo");

    let nav_resp = recv_from_actor(transport, &target_actor);
    save_cli_fixture("navigate_response.json", &nav_resp);

    // Wait for navigation to settle, reconnect
    std::thread::sleep(Duration::from_secs(2));
    drain_messages(transport, Duration::from_millis(500));
    drop(conn);

    let mut conn2 = connect();
    let transport2 = conn2.transport_mut();
    let (target_actor2, _console_actor2) = setup_target(transport2);

    // Reload
    transport2
        .send(&json!({"to": &target_actor2, "type": "reload"}))
        .expect("send reload");
    let reload_resp = recv_from_actor(transport2, &target_actor2);
    save_cli_fixture("reload_response.json", &reload_resp);
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_eval_immediate() {
    if !should_run_live() {
        return;
    }
    let mut conn = connect();
    let transport = conn.transport_mut();

    // Navigate to example.com first
    navigate_to_example_com(transport);

    let console = get_console_actor(transport);
    let (_immediate, _result) = record_eval(
        transport,
        &console,
        "document.title",
        Some("eval_immediate_response.json"),
        None,
    );
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_start_listeners() {
    if !should_run_live() {
        return;
    }
    let mut conn = connect();
    let transport = conn.transport_mut();

    let list_tabs = send_raw(transport, &json!({"to": "root", "type": "listTabs"}));
    let tab_actor = list_tabs["tabs"][0]["actor"].as_str().expect("tab actor");
    let target_resp = send_raw(transport, &json!({"to": tab_actor, "type": "getTarget"}));
    let console_actor = target_resp["frame"]["consoleActor"]
        .as_str()
        .expect("console actor");

    transport
        .send(&json!({
            "to": console_actor,
            "type": "startListeners",
            "listeners": ["PageError", "ConsoleAPI"]
        }))
        .expect("send startListeners");

    let resp = recv_from_actor(transport, console_actor);

    assert!(
        resp.get("from").is_some(),
        "startListeners response must have a 'from' field"
    );
    save_cli_fixture("start_listeners_response.json", &resp);
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_get_cached_messages() {
    if !should_run_live() {
        return;
    }
    let mut conn = connect();
    let transport = conn.transport_mut();

    // Navigate to example.com and generate console messages
    navigate_to_example_com(transport);

    let console_actor = get_console_actor(transport);

    // Start listeners first
    transport
        .send(&json!({
            "to": &console_actor,
            "type": "startListeners",
            "listeners": ["PageError", "ConsoleAPI"]
        }))
        .expect("send startListeners");
    let _ = recv_from_actor(transport, &console_actor);

    // Generate some console messages
    let _ = record_eval(
        transport,
        &console_actor,
        "console.log('hello'); console.warn('warn msg'); console.error('error msg'); 'done'",
        None,
        None,
    );

    // getCachedMessages
    transport
        .send(&json!({
            "to": &console_actor,
            "type": "getCachedMessages",
            "messageTypes": ["PageError", "ConsoleAPI"]
        }))
        .expect("send getCachedMessages");

    let resp = recv_from_actor(transport, &console_actor);

    assert!(
        resp.get("messages").and_then(Value::as_array).is_some(),
        "getCachedMessages must return messages array"
    );

    save_cli_fixture("get_cached_messages_response.json", &resp);
}

// ===========================================================================
// Part C: Eval-based command fixtures
// ===========================================================================

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_eval_string() {
    if !should_run_live() {
        return;
    }
    let mut conn = connect();
    let transport = conn.transport_mut();
    navigate_to_example_com(transport);
    let console = get_console_actor(transport);

    let (_imm, result) = record_eval(
        transport,
        &console,
        "document.title",
        None,
        Some("eval_result_string.json"),
    );

    assert!(
        result.get("result").and_then(Value::as_str).is_some(),
        "eval string must return a string result"
    );
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_eval_number() {
    if !should_run_live() {
        return;
    }
    let mut conn = connect();
    let transport = conn.transport_mut();
    navigate_to_example_com(transport);
    let console = get_console_actor(transport);

    let (_imm, result) = record_eval(
        transport,
        &console,
        "1 + 41",
        None,
        Some("eval_result_number.json"),
    );

    assert_eq!(result["result"], 42, "1 + 41 must equal 42");
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_eval_undefined() {
    if !should_run_live() {
        return;
    }
    let mut conn = connect();
    let transport = conn.transport_mut();
    navigate_to_example_com(transport);
    let console = get_console_actor(transport);

    let (_imm, result) = record_eval(
        transport,
        &console,
        "undefined",
        None,
        Some("eval_result_undefined.json"),
    );

    assert!(
        result["result"].get("type").is_some(),
        "undefined result should have a type field"
    );
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_eval_object() {
    if !should_run_live() {
        return;
    }
    let mut conn = connect();
    let transport = conn.transport_mut();
    navigate_to_example_com(transport);
    let console = get_console_actor(transport);

    let (_imm, result) = record_eval(
        transport,
        &console,
        "({a: 1, b: [2,3]})",
        None,
        Some("eval_result_object.json"),
    );

    assert_eq!(
        result["result"]["type"], "object",
        "object eval must return type=object"
    );
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_eval_exception() {
    if !should_run_live() {
        return;
    }
    let mut conn = connect();
    let transport = conn.transport_mut();
    navigate_to_example_com(transport);
    let console = get_console_actor(transport);

    let (_imm, result) = record_eval(
        transport,
        &console,
        "throw new Error('test error')",
        None,
        Some("eval_result_exception.json"),
    );

    assert_eq!(
        result["hasException"], true,
        "exception eval must set hasException=true"
    );
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_eval_long_string() {
    if !should_run_live() {
        return;
    }
    let mut conn = connect();
    let transport = conn.transport_mut();
    navigate_to_example_com(transport);
    let console = get_console_actor(transport);

    let (_imm, result) = record_eval(
        transport,
        &console,
        "'x'.repeat(50000)",
        None,
        Some("eval_result_long_string.json"),
    );

    // Firefox returns long strings as objects with type="longString"
    assert!(
        result["result"]["type"] == "longString"
            || result["result"].as_str().is_some_and(|s| s.len() > 1000),
        "long string should be a longString grip or a very long string"
    );
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_eval_null() {
    if !should_run_live() {
        return;
    }
    let mut conn = connect();
    let transport = conn.transport_mut();
    navigate_to_example_com(transport);
    let console = get_console_actor(transport);

    let (_imm, result) = record_eval(
        transport,
        &console,
        "null",
        None,
        Some("eval_result_dom_null.json"),
    );

    assert!(
        result["result"].is_null() || result["result"].get("type").is_some_and(|t| t == "null"),
        "null eval must return null or type=null"
    );
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_page_text() {
    if !should_run_live() {
        return;
    }
    let mut conn = connect();
    let transport = conn.transport_mut();
    navigate_to_example_com(transport);
    let console = get_console_actor(transport);

    let (_imm, result) = record_eval(
        transport,
        &console,
        "document.body.innerText",
        None,
        Some("eval_result_page_text.json"),
    );

    // Should return a string (possibly longString) containing "Example Domain"
    let is_string = result["result"].is_string();
    let is_long_string = result["result"]["type"] == "longString";
    assert!(
        is_string || is_long_string,
        "page text should be string or longString"
    );
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_wait_true() {
    if !should_run_live() {
        return;
    }
    let mut conn = connect();
    let transport = conn.transport_mut();
    navigate_to_example_com(transport);
    let console = get_console_actor(transport);

    let (_imm, result) = record_eval(
        transport,
        &console,
        "document.querySelector('h1') !== null",
        None,
        Some("eval_result_wait_true.json"),
    );

    assert_eq!(result["result"], true, "h1 should exist on example.com");
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_wait_false() {
    if !should_run_live() {
        return;
    }
    let mut conn = connect();
    let transport = conn.transport_mut();
    navigate_to_example_com(transport);
    let console = get_console_actor(transport);

    let (_imm, result) = record_eval(
        transport,
        &console,
        "document.querySelector('.never-appears') !== null",
        None,
        Some("eval_result_wait_false.json"),
    );

    assert_eq!(
        result["result"], false,
        ".never-appears should not exist on example.com"
    );
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_dom_text_single() {
    if !should_run_live() {
        return;
    }
    let mut conn = connect();
    let transport = conn.transport_mut();
    navigate_to_example_com(transport);
    let console = get_console_actor(transport);

    let js = "(function() { \
        var els = document.querySelectorAll('h1'); \
        if (els.length === 0) return null; \
        if (els.length === 1) return els[0].textContent; \
        return '__FF_RDP_JSON__' + JSON.stringify(Array.from(els, function(e) { return e.textContent; })); \
    })()";

    let (_imm, result) = record_eval(
        transport,
        &console,
        js,
        None,
        Some("eval_result_dom_text.json"),
    );

    assert!(
        result["result"].is_string(),
        "DOM text single should return a string"
    );
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_dom_html_single() {
    if !should_run_live() {
        return;
    }
    let mut conn = connect();
    let transport = conn.transport_mut();
    navigate_to_example_com(transport);
    let console = get_console_actor(transport);

    let js = "(function() { \
        var els = document.querySelectorAll('h1'); \
        if (els.length === 0) return null; \
        if (els.length === 1) return els[0].outerHTML; \
        return '__FF_RDP_JSON__' + JSON.stringify(Array.from(els, function(e) { return e.outerHTML; })); \
    })()";

    let (_imm, result) = record_eval(
        transport,
        &console,
        js,
        None,
        Some("eval_result_dom_single.json"),
    );

    assert!(
        result["result"].is_string(),
        "DOM HTML single should return a string"
    );
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_dom_text_multi() {
    if !should_run_live() {
        return;
    }
    let mut conn = connect();
    let transport = conn.transport_mut();
    navigate_to_example_com(transport);
    let console = get_console_actor(transport);

    let js = "(function() { \
        var els = document.querySelectorAll('p'); \
        if (els.length === 0) return null; \
        if (els.length === 1) return els[0].textContent; \
        return '__FF_RDP_JSON__' + JSON.stringify(Array.from(els, function(e) { return e.textContent; })); \
    })()";

    let (_imm, result) = record_eval(
        transport,
        &console,
        js,
        None,
        Some("eval_result_dom_multi_text.json"),
    );

    // example.com has 2 <p> elements, so should return __FF_RDP_JSON__ array
    assert!(
        result["result"].is_string(),
        "DOM text multi should return a string"
    );
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_dom_attrs() {
    if !should_run_live() {
        return;
    }
    let mut conn = connect();
    let transport = conn.transport_mut();
    navigate_to_example_com(transport);
    let console = get_console_actor(transport);

    let js = "(function() { \
        function attrs(e) { \
            var o = {}; \
            for (var i = 0; i < e.attributes.length; i++) { \
                o[e.attributes[i].name] = e.attributes[i].value; \
            } \
            return o; \
        } \
        var els = document.querySelectorAll('a'); \
        if (els.length === 0) return null; \
        if (els.length === 1) return '__FF_RDP_JSON__' + JSON.stringify(attrs(els[0])); \
        return '__FF_RDP_JSON__' + JSON.stringify(Array.from(els, attrs)); \
    })()";

    let (_imm, result) = record_eval(
        transport,
        &console,
        js,
        None,
        Some("eval_result_dom_attrs.json"),
    );

    assert!(
        result["result"].is_string(),
        "DOM attrs should return a string"
    );
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_click() {
    if !should_run_live() {
        return;
    }
    let mut conn = connect();
    let transport = conn.transport_mut();
    navigate_to_example_com(transport);
    let console = get_console_actor(transport);

    // Inject a test button
    let _ = record_eval(
        transport,
        &console,
        "(function() { \
            var btn = document.createElement('button'); \
            btn.className = 'test-btn'; \
            btn.textContent = 'Test Button'; \
            document.body.appendChild(btn); \
            return 'injected'; \
        })()",
        None,
        None,
    );

    // Click the button
    let js = "(function() { \
        var el = document.querySelector('.test-btn'); \
        if (!el) throw new Error('Element not found: .test-btn'); \
        el.click(); \
        return {clicked: true, tag: el.tagName, text: el.textContent.slice(0, 100)}; \
    })()";

    let (_imm, result) = record_eval(
        transport,
        &console,
        js,
        None,
        Some("eval_result_click.json"),
    );

    assert_eq!(
        result["result"]["type"], "object",
        "click result should be an object grip"
    );
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_click_missing() {
    if !should_run_live() {
        return;
    }
    let mut conn = connect();
    let transport = conn.transport_mut();
    navigate_to_example_com(transport);
    let console = get_console_actor(transport);

    let js = "(function() { \
        var el = document.querySelector('button.missing'); \
        if (!el) throw new Error('Element not found: button.missing'); \
        el.click(); \
        return {clicked: true, tag: el.tagName, text: el.textContent.slice(0, 100)}; \
    })()";

    let (_imm, result) = record_eval(
        transport,
        &console,
        js,
        None,
        Some("eval_result_element_not_found.json"),
    );

    assert_eq!(
        result["hasException"], true,
        "click on missing element should throw"
    );
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_type_text() {
    if !should_run_live() {
        return;
    }
    let mut conn = connect();
    let transport = conn.transport_mut();
    navigate_to_example_com(transport);
    let console = get_console_actor(transport);

    // Inject a test input
    let _ = record_eval(
        transport,
        &console,
        "(function() { \
            var inp = document.createElement('input'); \
            inp.type = 'email'; \
            inp.className = 'test-input'; \
            document.body.appendChild(inp); \
            return 'injected'; \
        })()",
        None,
        None,
    );

    let js = "(function() { \
        var el = document.querySelector('.test-input'); \
        if (!el) throw new Error('Element not found: .test-input'); \
        el.value = \"test@example.com\"; \
        el.dispatchEvent(new Event('input', {bubbles: true})); \
        el.dispatchEvent(new Event('change', {bubbles: true})); \
        return {typed: true, value: el.value}; \
    })()";

    let (_imm, result) = record_eval(transport, &console, js, None, Some("eval_result_type.json"));

    assert_eq!(
        result["result"]["type"], "object",
        "type result should be an object grip"
    );
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_cookies() {
    if !should_run_live() {
        return;
    }
    let mut conn = connect();
    let transport = conn.transport_mut();
    navigate_to_example_com(transport);
    let console = get_console_actor(transport);

    // Set cookies via JS so we have data to read
    let _ = record_eval(
        transport,
        &console,
        "document.cookie = 'session_id=abc123; path=/'; \
         document.cookie = 'theme=dark; path=/; expires=Fri, 31 Dec 2027 23:59:59 GMT'; 'set'",
        None,
        None,
    );

    // Now use the StorageActor protocol: getWatcher → watchResources → getStoreObjects
    let list_tabs = send_raw(transport, &json!({"to": "root", "type": "listTabs"}));
    let tab_actor = list_tabs["tabs"][0]["actor"].as_str().expect("tab actor");

    let watcher_resp = send_raw(transport, &json!({"to": tab_actor, "type": "getWatcher"}));
    let watcher = watcher_resp["actor"].as_str().expect("watcher actor");

    // watchResources("cookies") returns resources-available-array with cookie actor
    let watch_resp = send_raw(
        transport,
        &json!({
            "to": watcher,
            "type": "watchResources",
            "resourceTypes": ["cookies"]
        }),
    );

    if should_record() {
        save_cli_fixture("watch_resources_cookies_response.json", &watch_resp);
    }

    // Extract cookie actor and host from the response
    let cookie_actor = watch_resp["array"][0][1][0]["actor"]
        .as_str()
        .expect("cookie actor");
    let hosts = watch_resp["array"][0][1][0]["hosts"]
        .as_object()
        .expect("hosts map");
    let host = hosts.keys().next().expect("at least one host");

    // getStoreObjects with sessionString to avoid Firefox sort bug
    let store_resp = send_raw(
        transport,
        &json!({
            "to": cookie_actor,
            "type": "getStoreObjects",
            "host": host,
            "options": {"sessionString": "Session", "sortOn": "name"}
        }),
    );

    if should_record() {
        save_cli_fixture("get_store_objects_cookies_response.json", &store_resp);
    }

    let data = store_resp["data"].as_array().expect("data array");
    assert!(!data.is_empty(), "should have at least one cookie");
    assert!(
        data.iter().any(|c| c["name"] == "session_id"),
        "should contain session_id cookie"
    );

    // Best-effort unwatch
    let unwatch_resp = send_raw(
        transport,
        &json!({
            "to": watcher,
            "type": "unwatchResources",
            "resourceTypes": ["cookies"]
        }),
    );

    if should_record() {
        save_cli_fixture("unwatch_resources_response.json", &unwatch_resp);
    }
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_cookies_empty() {
    if !should_run_live() {
        return;
    }
    let mut conn = connect();
    let transport = conn.transport_mut();

    // Navigate to about:blank for empty cookies
    let list_tabs = send_raw(transport, &json!({"to": "root", "type": "listTabs"}));
    let tab_actor = list_tabs["tabs"][0]["actor"].as_str().expect("tab actor");
    let target_resp = send_raw(transport, &json!({"to": tab_actor, "type": "getTarget"}));
    let target_actor = target_resp["frame"]["actor"]
        .as_str()
        .expect("target actor");

    transport
        .send(&json!({
            "to": target_actor,
            "type": "navigateTo",
            "url": "about:blank"
        }))
        .expect("send navigateTo about:blank");

    std::thread::sleep(Duration::from_millis(500));
    drain_messages(transport, Duration::from_millis(500));
    drop(conn);

    // Reconnect and use StorageActor protocol
    let mut conn2 = connect();
    let transport = conn2.transport_mut();
    let list_tabs = send_raw(transport, &json!({"to": "root", "type": "listTabs"}));
    let tab_actor = list_tabs["tabs"][0]["actor"].as_str().expect("tab actor");

    let watcher_resp = send_raw(transport, &json!({"to": tab_actor, "type": "getWatcher"}));
    let watcher = watcher_resp["actor"].as_str().expect("watcher actor");

    let watch_resp = send_raw(
        transport,
        &json!({
            "to": watcher,
            "type": "watchResources",
            "resourceTypes": ["cookies"]
        }),
    );

    // Extract cookie actor and hosts.
    // about:blank may still have a host entry (e.g. from prior navigation).
    let cookie_actor = watch_resp["array"][0][1][0]["actor"]
        .as_str()
        .expect("cookie actor");
    let hosts = watch_resp["array"][0][1][0]["hosts"]
        .as_object()
        .expect("hosts map");

    // Always query a real host — never hand-craft fixture data. If no
    // hosts are present, the test cannot record the fixture; skip gracefully.
    if let Some(host) = hosts.keys().next() {
        let store_resp = send_raw(
            transport,
            &json!({
                "to": cookie_actor,
                "type": "getStoreObjects",
                "host": host,
                "options": {"sessionString": "Session", "sortOn": "name"}
            }),
        );

        if should_record() {
            save_cli_fixture("get_store_objects_cookies_empty_response.json", &store_resp);
        }

        let data = store_resp["data"].as_array().expect("data array");
        assert!(data.is_empty(), "cookies on about:blank should be empty");
    } else {
        println!("  [skip] no hosts on about:blank — cannot record empty cookie fixture");
    }

    // Best-effort unwatch
    let _ = send_raw(
        transport,
        &json!({
            "to": watcher,
            "type": "unwatchResources",
            "resourceTypes": ["cookies"]
        }),
    );
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_storage_all() {
    if !should_run_live() {
        return;
    }
    let mut conn = connect();
    let transport = conn.transport_mut();
    navigate_to_example_com(transport);
    let console = get_console_actor(transport);

    // Set storage items
    let _ = record_eval(
        transport,
        &console,
        "localStorage.setItem('token', 'abc'); localStorage.setItem('theme', 'dark'); 'set'",
        None,
        None,
    );

    let js = "(function() { \
        var s = localStorage; \
        var obj = {}; \
        for (var i = 0; i < s.length; i++) { \
            var k = s.key(i); \
            obj[k] = s.getItem(k); \
        } \
        return JSON.stringify(obj); \
    })()";

    let (_imm, result) = record_eval(
        transport,
        &console,
        js,
        None,
        Some("eval_result_storage.json"),
    );

    let result_str = result["result"].as_str().unwrap_or("{}");
    let storage: Value = serde_json::from_str(result_str).expect("storage must be valid JSON");
    assert!(storage.is_object(), "storage result should be an object");
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_storage_key() {
    if !should_run_live() {
        return;
    }
    let mut conn = connect();
    let transport = conn.transport_mut();
    navigate_to_example_com(transport);
    let console = get_console_actor(transport);

    // Ensure key exists
    let _ = record_eval(
        transport,
        &console,
        "localStorage.setItem('token', 'abc'); 'set'",
        None,
        None,
    );

    let js = r#"(function() { var v = localStorage.getItem("token"); return v; })()"#;

    let (_imm, result) = record_eval(
        transport,
        &console,
        js,
        None,
        Some("eval_result_storage_key.json"),
    );

    assert!(
        result["result"].is_string(),
        "storage key result should be a string"
    );
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_storage_null() {
    if !should_run_live() {
        return;
    }
    let mut conn = connect();
    let transport = conn.transport_mut();
    navigate_to_example_com(transport);
    let console = get_console_actor(transport);

    let js =
        r#"(function() { var v = localStorage.getItem("nonexistent_key_12345"); return v; })()"#;

    let (_imm, result) = record_eval(
        transport,
        &console,
        js,
        None,
        Some("eval_result_storage_null.json"),
    );

    assert!(
        result["result"].is_null() || result["result"].get("type").is_some_and(|t| t == "null"),
        "nonexistent storage key should return null"
    );
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_screenshot() {
    if !should_run_live() {
        return;
    }
    let mut conn = connect();
    let transport = conn.transport_mut();
    navigate_to_example_com(transport);
    let console = get_console_actor(transport);

    let js = "(function() { \
        var w = window.innerWidth || document.documentElement.clientWidth || 800; \
        var h = window.innerHeight || document.documentElement.clientHeight || 600; \
        var canvas = document.createElement('canvas'); \
        canvas.width = w; \
        canvas.height = h; \
        var ctx = canvas.getContext('2d'); \
        if (!ctx || typeof ctx.drawWindow !== 'function') { return null; } \
        ctx.drawWindow(window, 0, 0, w, h, 'rgb(255,255,255)'); \
        return canvas.toDataURL('image/png'); \
    })()";

    let (_imm, result) = record_eval(
        transport,
        &console,
        js,
        None,
        Some("eval_result_screenshot.json"),
    );

    // In headless mode, drawWindow may not be available → null result
    // Either null or a data URL is acceptable
    let r = &result["result"];
    assert!(
        r.is_null()
            || r.is_string()
            || r.get("type")
                .is_some_and(|t| t == "null" || t == "longString"),
        "screenshot should be null, string, or longString"
    );
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_screenshot_null() {
    if !should_run_live() {
        return;
    }
    let mut conn = connect();
    let transport = conn.transport_mut();
    navigate_to_example_com(transport);
    let console = get_console_actor(transport);

    // Force a null screenshot by using an impossible canvas operation
    let js = "(function() { return null; })()";

    let (_imm, result) = record_eval(
        transport,
        &console,
        js,
        None,
        Some("eval_result_screenshot_null.json"),
    );

    assert!(
        result["result"].is_null() || result["result"].get("type").is_some_and(|t| t == "null"),
        "should return null"
    );
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_perf_timing() {
    if !should_run_live() {
        return;
    }
    let mut conn = connect();
    let transport = conn.transport_mut();
    navigate_to_example_com(transport);
    let console = get_console_actor(transport);

    let js = "JSON.stringify(performance.getEntriesByType(\"resource\").map(e => ({name: e.name, initiatorType: e.initiatorType, duration: Math.round(e.duration * 100) / 100, transferSize: e.transferSize, encodedBodySize: e.encodedBodySize, decodedBodySize: e.decodedBodySize, startTime: Math.round(e.startTime * 100) / 100, responseEnd: Math.round(e.responseEnd * 100) / 100, protocol: e.nextHopProtocol})))";

    let (_imm, result) = record_eval(
        transport,
        &console,
        js,
        None,
        Some("eval_result_perf_timing.json"),
    );

    // Should return a JSON string (possibly longString)
    let r = &result["result"];
    assert!(
        r.is_string() || r.get("type").is_some_and(|t| t == "longString"),
        "perf timing should be string or longString"
    );
}

// ===========================================================================
// Part D: Edge cases - longString/substring, network resources
// ===========================================================================

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_long_string_substring() {
    if !should_run_live() {
        return;
    }
    let mut conn = connect();
    let transport = conn.transport_mut();
    navigate_to_example_com(transport);
    let console = get_console_actor(transport);

    // Generate a long page text (inject content)
    let _ = record_eval(
        transport,
        &console,
        "(function() { var d = document.createElement('div'); d.textContent = 'x'.repeat(50000); document.body.appendChild(d); return 'injected'; })()",
        None,
        None,
    );

    // Get page text as longString
    let (_imm, result) = record_eval(transport, &console, "document.body.innerText", None, None);

    // If it's a longString, fetch the substring
    if result["result"]["type"] == "longString" {
        let long_actor = result["result"]["actor"]
            .as_str()
            .expect("longString actor");
        let length = result["result"]["length"].as_u64().unwrap_or(1000);

        transport
            .send(&json!({
                "to": long_actor,
                "type": "substring",
                "start": 0,
                "end": length
            }))
            .expect("send substring");

        let substr_resp = recv_from_actor(transport, long_actor);
        save_cli_fixture("substring_page_text_response.json", &substr_resp);
    }

    // Screenshot as longString
    let js_screenshot = "(function() { \
        var w = window.innerWidth || document.documentElement.clientWidth || 800; \
        var h = window.innerHeight || document.documentElement.clientHeight || 600; \
        var canvas = document.createElement('canvas'); \
        canvas.width = w; \
        canvas.height = h; \
        var ctx = canvas.getContext('2d'); \
        if (!ctx || typeof ctx.drawWindow !== 'function') { return null; } \
        ctx.drawWindow(window, 0, 0, w, h, 'rgb(255,255,255)'); \
        return canvas.toDataURL('image/png'); \
    })()";

    let (_imm, screenshot_result) = record_eval(transport, &console, js_screenshot, None, None);

    if screenshot_result["result"]["type"] == "longString" {
        let long_actor = screenshot_result["result"]["actor"]
            .as_str()
            .expect("screenshot longString actor");
        let length = screenshot_result["result"]["length"]
            .as_u64()
            .unwrap_or(1000);

        transport
            .send(&json!({
                "to": long_actor,
                "type": "substring",
                "start": 0,
                "end": length
            }))
            .expect("send substring");

        let substr_resp = recv_from_actor(transport, long_actor);
        save_cli_fixture("substring_screenshot_response.json", &substr_resp);
    }
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_page_text_long() {
    if !should_run_live() {
        return;
    }
    let mut conn = connect();
    let transport = conn.transport_mut();
    navigate_to_example_com(transport);
    let console = get_console_actor(transport);

    // Inject long content
    let _ = record_eval(
        transport,
        &console,
        "(function() { var d = document.createElement('div'); d.textContent = 'longtext '.repeat(10000); document.body.appendChild(d); return 'injected'; })()",
        None,
        None,
    );

    let (_imm, result) = record_eval(
        transport,
        &console,
        "document.body.innerText",
        None,
        Some("eval_result_page_text_long.json"),
    );

    let r = &result["result"];
    assert!(
        r.is_string() || r.get("type").is_some_and(|t| t == "longString"),
        "long page text should be string or longString"
    );
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_screenshot_longstring() {
    if !should_run_live() {
        return;
    }
    let mut conn = connect();
    let transport = conn.transport_mut();
    navigate_to_example_com(transport);
    let console = get_console_actor(transport);

    let js = "(function() { \
        var w = window.innerWidth || document.documentElement.clientWidth || 800; \
        var h = window.innerHeight || document.documentElement.clientHeight || 600; \
        var canvas = document.createElement('canvas'); \
        canvas.width = w; \
        canvas.height = h; \
        var ctx = canvas.getContext('2d'); \
        if (!ctx || typeof ctx.drawWindow !== 'function') { return null; } \
        ctx.drawWindow(window, 0, 0, w, h, 'rgb(255,255,255)'); \
        return canvas.toDataURL('image/png'); \
    })()";

    let (_imm, result) = record_eval(
        transport,
        &console,
        js,
        None,
        Some("eval_result_screenshot_longstring.json"),
    );

    // May be null in headless, longString in headed mode
    let r = &result["result"];
    assert!(
        r.is_null()
            || r.is_string()
            || r.get("type")
                .is_some_and(|t| t == "null" || t == "longString"),
        "screenshot should be null, string, or longString"
    );
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_cached_longstring() {
    if !should_run_live() {
        return;
    }
    let mut conn = connect();
    let transport = conn.transport_mut();
    navigate_to_example_com(transport);
    let console = get_console_actor(transport);

    // Performance API can return longString for large result sets
    let js = "JSON.stringify(performance.getEntriesByType('resource').map(e => ({name: e.name, initiatorType: e.initiatorType, duration: Math.round(e.duration * 100) / 100, transferSize: e.transferSize, encodedBodySize: e.encodedBodySize, decodedBodySize: e.decodedBodySize, startTime: Math.round(e.startTime * 100) / 100, responseEnd: Math.round(e.responseEnd * 100) / 100, protocol: e.nextHopProtocol})))";

    let (_imm, result) = record_eval(
        transport,
        &console,
        js,
        None,
        Some("eval_result_cached_longstring.json"),
    );

    let r = &result["result"];
    assert!(
        r.is_string() || r.get("type").is_some_and(|t| t == "longString"),
        "cached perf should be string or longString"
    );
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_cached_exception() {
    if !should_run_live() {
        return;
    }
    let mut conn = connect();
    let transport = conn.transport_mut();
    navigate_to_example_com(transport);
    let console = get_console_actor(transport);

    // Force an exception in Performance API context
    let js = "(function() { throw new Error('Performance API error simulation'); })()";

    let (_imm, result) = record_eval(
        transport,
        &console,
        js,
        None,
        Some("eval_result_cached_exception.json"),
    );

    assert_eq!(
        result["hasException"], true,
        "should be an exception result"
    );
}

#[test]
#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]
fn live_network_resources() {
    if !should_run_live() {
        return;
    }
    let mut conn = connect();
    let transport = conn.transport_mut();

    let list_tabs = send_raw(transport, &json!({"to": "root", "type": "listTabs"}));
    let tab_actor = list_tabs["tabs"][0]["actor"]
        .as_str()
        .expect("tab actor")
        .to_owned();

    let target_resp = send_raw(transport, &json!({"to": tab_actor, "type": "getTarget"}));
    let target_actor = target_resp["frame"]["actor"]
        .as_str()
        .expect("target actor")
        .to_owned();

    let watcher_resp = send_raw(transport, &json!({"to": tab_actor, "type": "getWatcher"}));
    let watcher_actor = watcher_resp["actor"]
        .as_str()
        .expect("watcher actor")
        .to_owned();

    // Watch network events
    transport
        .send(&json!({
            "to": &watcher_actor,
            "type": "watchResources",
            "resourceTypes": ["network-event"]
        }))
        .expect("send watchResources");

    // Drain initial events
    drain_messages(transport, Duration::from_secs(2));

    // Navigate to trigger network activity
    transport
        .send(&json!({
            "to": &target_actor,
            "type": "navigateTo",
            "url": "https://example.com/"
        }))
        .expect("send navigateTo");

    std::thread::sleep(Duration::from_secs(3));
    let events = drain_messages(transport, Duration::from_secs(3));

    let mut net_actor = None;
    let mut saved_available = false;
    let mut saved_updated = false;
    for msg in &events {
        let msg_type = msg.get("type").and_then(Value::as_str).unwrap_or_default();

        if msg_type == "resources-available-array" || msg_type == "resource-available-form" {
            // Save only the first occurrence so we capture a representative example
            // without overwriting it with every subsequent network event message.
            if !saved_available {
                save_cli_fixture("resources_available_network.json", msg);
                saved_available = true;
            }

            // Extract network event actor
            if let Some(array) = msg.get("array").and_then(Value::as_array) {
                for sub in array {
                    if let Some(sub_arr) = sub.as_array()
                        && sub_arr.len() == 2
                        && let Some(resources) = sub_arr[1].as_array()
                    {
                        for res in resources {
                            if let Some(actor) = res.get("actor").and_then(Value::as_str)
                                && actor.contains("netEvent")
                                && net_actor.is_none()
                            {
                                net_actor = Some(actor.to_owned());
                            }
                        }
                    }
                }
            }
        }

        if msg_type == "resources-updated-array" || msg_type == "resource-updated-form" {
            // Save only the first occurrence.
            if !saved_updated {
                save_cli_fixture("resources_updated_network.json", msg);
                saved_updated = true;
            }
        }
    }

    // If we got a network event actor, also record network details
    if net_actor.is_some() {
        // Need fresh connection for detail queries
        drop(conn);
        let mut conn2 = connect();
        let t2 = conn2.transport_mut();

        // Re-setup and navigate to get fresh actors
        let list_tabs2 = send_raw(t2, &json!({"to": "root", "type": "listTabs"}));
        let tab_actor2 = list_tabs2["tabs"][0]["actor"]
            .as_str()
            .expect("tab actor")
            .to_owned();
        let target_resp2 = send_raw(t2, &json!({"to": &tab_actor2, "type": "getTarget"}));
        let target_actor2 = target_resp2["frame"]["actor"]
            .as_str()
            .expect("target actor")
            .to_owned();
        let watcher_resp2 = send_raw(t2, &json!({"to": &tab_actor2, "type": "getWatcher"}));
        let watcher_actor2 = watcher_resp2["actor"]
            .as_str()
            .expect("watcher actor")
            .to_owned();

        // Watch and navigate again
        t2.send(&json!({
            "to": &watcher_actor2,
            "type": "watchResources",
            "resourceTypes": ["network-event"]
        }))
        .expect("send watchResources");
        drain_messages(t2, Duration::from_secs(2));

        t2.send(&json!({
            "to": &target_actor2,
            "type": "navigateTo",
            "url": "https://example.com/"
        }))
        .expect("send navigateTo");

        std::thread::sleep(Duration::from_secs(3));
        let events2 = drain_messages(t2, Duration::from_secs(3));

        let mut fresh_net_actor = None;
        for msg in &events2 {
            let msg_type = msg.get("type").and_then(Value::as_str).unwrap_or_default();
            if msg_type == "resources-available-array"
                && let Some(array) = msg.get("array").and_then(Value::as_array)
            {
                for sub in array {
                    if let Some(sub_arr) = sub.as_array()
                        && sub_arr.len() == 2
                        && let Some(resources) = sub_arr[1].as_array()
                    {
                        for res in resources {
                            if let Some(actor) = res.get("actor").and_then(Value::as_str)
                                && actor.contains("netEvent")
                                && fresh_net_actor.is_none()
                                && res
                                    .get("cause")
                                    .and_then(|c| c.get("type"))
                                    .and_then(Value::as_str)
                                    == Some("document")
                            {
                                fresh_net_actor = Some(actor.to_owned());
                            }
                        }
                    }
                }
            }
        }

        if let Some(fresh) = fresh_net_actor {
            // getRequestHeaders
            t2.send(&json!({"to": &fresh, "type": "getRequestHeaders"}))
                .expect("send");
            let resp = recv_from_actor(t2, &fresh);
            save_cli_fixture("get_request_headers_response.json", &resp);

            // getResponseHeaders
            t2.send(&json!({"to": &fresh, "type": "getResponseHeaders"}))
                .expect("send");
            let resp = recv_from_actor(t2, &fresh);
            save_cli_fixture("get_response_headers_response.json", &resp);

            // getResponseContent
            t2.send(&json!({"to": &fresh, "type": "getResponseContent"}))
                .expect("send");
            let resp = recv_from_actor(t2, &fresh);
            save_cli_fixture("get_response_content_response.json", &resp);

            // getEventTimings
            t2.send(&json!({"to": &fresh, "type": "getEventTimings"}))
                .expect("send");
            let resp = recv_from_actor(t2, &fresh);
            save_cli_fixture("get_event_timings_response.json", &resp);
        }
    }
}

// ===========================================================================
// Shared helpers (not exported — internal to this test file)
// ===========================================================================

/// Run listTabs + getTarget and return (target_actor, console_actor).
fn setup_target(transport: &mut RdpTransport) -> (String, String) {
    let list_tabs = send_raw(transport, &json!({"to": "root", "type": "listTabs"}));
    let tab_actor = list_tabs["tabs"][0]["actor"].as_str().expect("tab actor");
    let target_resp = send_raw(transport, &json!({"to": tab_actor, "type": "getTarget"}));
    let frame = &target_resp["frame"];
    let target_actor = frame["actor"].as_str().expect("target actor").to_owned();
    let console_actor = frame["consoleActor"]
        .as_str()
        .expect("console actor")
        .to_owned();
    (target_actor, console_actor)
}

/// Send a `navigateTo` request for <https://example.com/>, wait for navigation
/// to settle, then drain any pending messages from the transport.
///
/// This does **not** reconnect. Actors acquired before this call remain on
/// the same connection. Callers that need fresh actors after navigation must
/// drop their connection and call `connect()` separately.
fn navigate_to_example_com(transport: &mut RdpTransport) {
    let (target_actor, _console) = setup_target(transport);

    transport
        .send(&json!({
            "to": &target_actor,
            "type": "navigateTo",
            "url": "https://example.com/"
        }))
        .expect("send navigateTo");

    // Wait for navigation to settle
    std::thread::sleep(Duration::from_secs(2));
    drain_messages(transport, Duration::from_millis(500));
}

/// Get a console actor from a fresh listTabs + getTarget.
fn get_console_actor(transport: &mut RdpTransport) -> String {
    let (_target, console) = setup_target(transport);
    console
}
