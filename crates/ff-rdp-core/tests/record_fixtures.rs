/// Run with: cargo test -p ff-rdp-core --test record_fixtures -- --ignored --nocapture
///
/// Records real Firefox RDP responses to stdout for use as test fixtures.
/// Requires Firefox running with --start-debugger-server 6000.
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

fn send_raw(transport: &mut RdpTransport, request: &Value) -> Value {
    transport.send(request).expect("send");
    transport.recv().expect("recv")
}

/// Read messages until we get a response with the expected `from` actor,
/// discarding events along the way.
fn recv_from_actor(transport: &mut RdpTransport, expected_from: &str) -> Value {
    loop {
        let msg = transport.recv().expect("recv");
        let from = msg.get("from").and_then(Value::as_str).unwrap_or_default();
        if from == expected_from {
            // Check it's not an event we want to skip
            let msg_type = msg.get("type").and_then(Value::as_str).unwrap_or_default();
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

/// Read messages until we get an evaluationResult with matching resultID.
fn recv_eval_result(transport: &mut RdpTransport, result_id: &str) -> Value {
    loop {
        let msg = transport.recv().expect("recv");
        let msg_type = msg.get("type").and_then(Value::as_str).unwrap_or_default();
        let msg_id = msg
            .get("resultID")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if msg_type == "evaluationResult" && msg_id == result_id {
            return msg;
        }
        let from = msg.get("from").and_then(Value::as_str).unwrap_or_default();
        print_fixture(
            &format!("(discarded while waiting for eval: from={from} type={msg_type})"),
            &msg,
        );
    }
}

#[test]
#[ignore = "requires a live Firefox instance on port 6000"]
fn record_all_fixtures() {
    let mut conn = connect();
    let transport = conn.transport_mut();

    // 1. listTabs
    let list_tabs_resp = send_raw(transport, &json!({"to": "root", "type": "listTabs"}));
    print_fixture("list_tabs_response", &list_tabs_resp);

    let tab_actor = list_tabs_resp["tabs"][0]["actor"]
        .as_str()
        .expect("tab actor")
        .to_owned();
    println!("# Using tab actor: {tab_actor}\n");

    // 2. getTarget (before navigation — on about:sessionrestore)
    let get_target_resp = send_raw(transport, &json!({"to": tab_actor, "type": "getTarget"}));
    print_fixture("get_target_response", &get_target_resp);

    let frame = &get_target_resp["frame"];
    let target_actor = frame["actor"].as_str().expect("target actor").to_owned();
    let console_actor = frame["consoleActor"]
        .as_str()
        .expect("console actor")
        .to_owned();
    println!("# Target actor: {target_actor}");
    println!("# Console actor: {console_actor}\n");

    // 3. Navigate to example.com — this will invalidate current actors
    transport
        .send(&json!({
            "to": target_actor,
            "type": "navigateTo",
            "url": "https://example.com/"
        }))
        .expect("send navigateTo");

    // Drain all messages until navigation settles
    println!("# --- Navigation events ---");
    std::thread::sleep(Duration::from_secs(2));
    // Drain remaining messages
    while let Ok(msg) = transport.recv() {
        let msg_type = msg.get("type").and_then(Value::as_str).unwrap_or_default();
        let from = msg.get("from").and_then(Value::as_str).unwrap_or_default();
        print_fixture(&format!("(nav event: from={from} type={msg_type})"), &msg);
    }

    // 4. Reconnect and get fresh target after navigation
    drop(conn);
    println!("\n# --- Reconnecting after navigation ---\n");
    let mut conn2 = connect();
    let transport2 = conn2.transport_mut();

    let list_tabs_resp2 = send_raw(transport2, &json!({"to": "root", "type": "listTabs"}));
    print_fixture("list_tabs_response (after nav)", &list_tabs_resp2);

    let tab_actor2 = list_tabs_resp2["tabs"][0]["actor"]
        .as_str()
        .expect("tab actor")
        .to_owned();

    let get_target_resp2 = send_raw(transport2, &json!({"to": tab_actor2, "type": "getTarget"}));
    print_fixture("get_target_response (after nav)", &get_target_resp2);

    let frame2 = &get_target_resp2["frame"];
    let target_actor2 = frame2["actor"].as_str().expect("target actor").to_owned();
    let console_actor2 = frame2["consoleActor"]
        .as_str()
        .expect("console actor")
        .to_owned();
    println!("# New target actor: {target_actor2}");
    println!("# New console actor: {console_actor2}\n");

    // 5. evaluateJSAsync — string result (document.title)
    transport2
        .send(&json!({
            "to": console_actor2,
            "type": "evaluateJSAsync",
            "text": "document.title",
            "eager": false
        }))
        .expect("send eval");

    let eval_immediate1 = recv_from_actor(transport2, &console_actor2);
    print_fixture(
        "evaluate_js_async_immediate (document.title)",
        &eval_immediate1,
    );

    let result_id1 = eval_immediate1["resultID"]
        .as_str()
        .expect("resultID")
        .to_owned();
    let eval_result1 = recv_eval_result(transport2, &result_id1);
    print_fixture("evaluation_result (document.title)", &eval_result1);

    // 6. evaluateJSAsync — number
    transport2
        .send(&json!({
            "to": console_actor2,
            "type": "evaluateJSAsync",
            "text": "1 + 41",
            "eager": false
        }))
        .expect("send eval");

    let eval_immediate2 = recv_from_actor(transport2, &console_actor2);
    print_fixture("evaluate_js_async_immediate (1+41)", &eval_immediate2);

    let result_id2 = eval_immediate2["resultID"]
        .as_str()
        .expect("resultID")
        .to_owned();
    let eval_result2 = recv_eval_result(transport2, &result_id2);
    print_fixture("evaluation_result (1+41)", &eval_result2);

    // 7. evaluateJSAsync — undefined
    transport2
        .send(&json!({
            "to": console_actor2,
            "type": "evaluateJSAsync",
            "text": "undefined",
            "eager": false
        }))
        .expect("send eval");

    let eval_immediate3 = recv_from_actor(transport2, &console_actor2);
    print_fixture("evaluate_js_async_immediate (undefined)", &eval_immediate3);

    let result_id3 = eval_immediate3["resultID"]
        .as_str()
        .expect("resultID")
        .to_owned();
    let eval_result3 = recv_eval_result(transport2, &result_id3);
    print_fixture("evaluation_result (undefined)", &eval_result3);

    // 8. evaluateJSAsync — object
    transport2
        .send(&json!({
            "to": console_actor2,
            "type": "evaluateJSAsync",
            "text": "({a: 1, b: [2,3]})",
            "eager": false
        }))
        .expect("send eval");

    let eval_immediate4 = recv_from_actor(transport2, &console_actor2);
    print_fixture("evaluate_js_async_immediate (object)", &eval_immediate4);

    let result_id4 = eval_immediate4["resultID"]
        .as_str()
        .expect("resultID")
        .to_owned();
    let eval_result4 = recv_eval_result(transport2, &result_id4);
    print_fixture("evaluation_result (object)", &eval_result4);

    // 9. evaluateJSAsync — exception
    transport2
        .send(&json!({
            "to": console_actor2,
            "type": "evaluateJSAsync",
            "text": "throw new Error('test error')",
            "eager": false
        }))
        .expect("send eval");

    let eval_immediate5 = recv_from_actor(transport2, &console_actor2);
    print_fixture("evaluate_js_async_immediate (exception)", &eval_immediate5);

    let result_id5 = eval_immediate5["resultID"]
        .as_str()
        .expect("resultID")
        .to_owned();
    let eval_result5 = recv_eval_result(transport2, &result_id5);
    print_fixture("evaluation_result (exception)", &eval_result5);

    // 10. evaluateJSAsync — long string
    transport2
        .send(&json!({
            "to": console_actor2,
            "type": "evaluateJSAsync",
            "text": "'x'.repeat(50000)",
            "eager": false
        }))
        .expect("send eval");

    let eval_immediate6 = recv_from_actor(transport2, &console_actor2);
    print_fixture(
        "evaluate_js_async_immediate (long string)",
        &eval_immediate6,
    );

    let result_id6 = eval_immediate6["resultID"]
        .as_str()
        .expect("resultID")
        .to_owned();
    let eval_result6 = recv_eval_result(transport2, &result_id6);
    print_fixture("evaluation_result (long string)", &eval_result6);

    // 11. reload
    transport2
        .send(&json!({
            "to": target_actor2,
            "type": "reload"
        }))
        .expect("send reload");
    let reload_resp = recv_from_actor(transport2, &target_actor2);
    print_fixture("reload_response", &reload_resp);

    // 12. getWatcher
    let watcher_resp = send_raw(transport2, &json!({"to": tab_actor2, "type": "getWatcher"}));
    print_fixture("get_watcher_response", &watcher_resp);

    println!("# Done recording fixtures!");
}
