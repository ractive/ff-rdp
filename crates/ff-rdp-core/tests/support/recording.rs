//! Shared helpers for live Firefox tests that optionally record fixtures.
//!
//! Two env vars control behaviour:
//!
//! - `FF_RDP_LIVE_TESTS=1` — run live tests against a real Firefox instance
//! - `FF_RDP_LIVE_TESTS_RECORD=1` — implies live tests; also writes every
//!   validated RDP response to the corresponding fixture file on disk
//!
//! # Usage
//!
//! ```rust,ignore
//! if !should_run_live() { return; }
//! // … exercise Firefox …
//! save_cli_fixture("eval_result_string.json", &value);
//! ```

use std::path::{Path, PathBuf};

use regex::Regex;
use serde_json::{Value, json};

use ff_rdp_core::RdpTransport;

/// Returns `true` when live tests should execute.
pub fn should_run_live() -> bool {
    std::env::var("FF_RDP_LIVE_TESTS").is_ok() || std::env::var("FF_RDP_LIVE_TESTS_RECORD").is_ok()
}

/// Returns `true` when fixtures should be written to disk.
pub fn should_record() -> bool {
    std::env::var("FF_RDP_LIVE_TESTS_RECORD").is_ok()
}

/// Override for the Firefox debugger port (default 6000).
pub fn firefox_port() -> u16 {
    std::env::var("FF_RDP_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(6000)
}

// ---------------------------------------------------------------------------
// Fixture path helpers
// ---------------------------------------------------------------------------

fn core_fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

fn cli_fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("ff-rdp-cli")
        .join("tests")
        .join("fixtures")
}

// ---------------------------------------------------------------------------
// Normalize
// ---------------------------------------------------------------------------

/// Normalize actor connection IDs (`conn\d+` → `conn0`) for cross-fixture
/// consistency, leaving everything else (timestamps, window IDs, etc.) as-is.
pub fn normalize_fixture(value: &Value) -> Value {
    let re = Regex::new(r"conn\d+").expect("valid regex");
    normalize_value(value, &re)
}

fn normalize_value(value: &Value, re: &Regex) -> Value {
    match value {
        Value::String(s) => Value::String(re.replace_all(s, "conn0").into_owned()),
        Value::Array(arr) => Value::Array(arr.iter().map(|v| normalize_value(v, re)).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), normalize_value(v, re)))
                .collect(),
        ),
        other => other.clone(),
    }
}

// ---------------------------------------------------------------------------
// Save helpers
// ---------------------------------------------------------------------------

fn write_fixture(dir: &Path, name: &str, value: &Value) {
    if !should_record() {
        return;
    }
    std::fs::create_dir_all(dir)
        .unwrap_or_else(|e| panic!("create fixture dir {}: {e}", dir.display()));
    let path = dir.join(name);
    let normalized = normalize_fixture(value);
    let json = serde_json::to_string_pretty(&normalized)
        .unwrap_or_else(|e| panic!("serialize fixture {name}: {e}"));
    // Trailing newline for POSIX-friendly files
    let contents = format!("{json}\n");
    std::fs::write(&path, contents)
        .unwrap_or_else(|e| panic!("write fixture {}: {e}", path.display()));
    println!("  [recorded] {}", path.display());
}

/// Write a fixture to `ff-rdp-cli/tests/fixtures/{name}`.
pub fn save_cli_fixture(name: &str, value: &Value) {
    write_fixture(&cli_fixtures_dir(), name, value);
}

/// Write a fixture to `ff-rdp-core/tests/fixtures/{name}`.
pub fn save_core_fixture(name: &str, value: &Value) {
    write_fixture(&core_fixtures_dir(), name, value);
}

// ---------------------------------------------------------------------------
// RDP helpers
// ---------------------------------------------------------------------------

/// Read messages until we get one with `"from"` matching `expected_from`,
/// discarding known async events along the way.
pub fn recv_from_actor(transport: &mut RdpTransport, expected_from: &str) -> Value {
    loop {
        let msg = transport.recv().expect("recv");
        let from = msg.get("from").and_then(Value::as_str).unwrap_or_default();
        if from == expected_from {
            let msg_type = msg.get("type").and_then(Value::as_str).unwrap_or_default();
            if msg_type == "frameUpdate"
                || msg_type == "tabNavigated"
                || msg_type == "tabListChanged"
            {
                continue;
            }
            return msg;
        }
    }
}

/// Read messages until we get an `evaluationResult` with matching `resultID`.
pub fn recv_eval_result(transport: &mut RdpTransport, result_id: &str) -> Value {
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
    }
}

/// Send `evaluateJSAsync` and return `(immediate_ack, evaluation_result)`.
///
/// If `should_record()`, both responses are also saved to the CLI fixture
/// directory using the provided fixture names.
pub fn record_eval(
    transport: &mut RdpTransport,
    console_actor: &str,
    js: &str,
    immediate_fixture: Option<&str>,
    result_fixture: Option<&str>,
) -> (Value, Value) {
    transport
        .send(&json!({
            "to": console_actor,
            "type": "evaluateJSAsync",
            "text": js,
            "eager": false
        }))
        .expect("send evaluateJSAsync");

    let immediate = recv_from_actor(transport, console_actor);
    let result_id = immediate["resultID"]
        .as_str()
        .expect("resultID in immediate ack")
        .to_owned();
    let result = recv_eval_result(transport, &result_id);

    if let Some(name) = immediate_fixture {
        save_cli_fixture(name, &immediate);
    }
    if let Some(name) = result_fixture {
        save_cli_fixture(name, &result);
    }

    (immediate, result)
}

/// Send a raw RDP request and read one response.
pub fn send_raw(transport: &mut RdpTransport, request: &Value) -> Value {
    transport.send(request).expect("send");
    transport.recv().expect("recv")
}

/// Drain all available messages with a short timeout, returning them.
pub fn drain_messages(transport: &mut RdpTransport, timeout: std::time::Duration) -> Vec<Value> {
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
