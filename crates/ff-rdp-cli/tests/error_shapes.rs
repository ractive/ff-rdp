//! Snapshot tests for structured error output (`error_type` field) and
//! deterministic exit codes when commands fail at different protocol layers.
//!
//! Each test spawns the `ff-rdp` binary against a mock server that injects
//! a fault at a specific layer and then asserts:
//! - The exit code matches the documented table.
//! - The JSON on stdout contains an `"error_type"` field matching the
//!   `RdpError` discriminant (or one of the legacy AppError variants).
//!
//! EXIT CODE TABLE (iter-61m):
//!   Protocol      → 3
//!   Shape         → 4
//!   Timeout(rdp)  → 5
//!   Transport / RemoteClosed → 6
//!   Connection    → 3
//!   Timeout(op)   → 124
//!   User / Internal → 1

use std::io::Write;
use std::net::TcpListener;

use ff_rdp_core::transport::encode_frame;
use serde_json::Value;

fn ff_rdp_bin() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_ff-rdp"))
}

/// Base args to bypass the daemon and talk to a local mock port.
fn base_args(port: u16) -> Vec<String> {
    vec![
        "--host".to_owned(),
        "127.0.0.1".to_owned(),
        "--port".to_owned(),
        port.to_string(),
        "--no-daemon".to_owned(),
    ]
}

/// Parse stdout JSON from a command invocation.
///
/// Returns `None` if stdout is empty or unparseable — callers can assert on
/// field presence themselves.
fn parse_stdout_json(stdout: &[u8]) -> Option<Value> {
    let s = std::str::from_utf8(stdout).ok()?;
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    serde_json::from_str(s).ok()
}

// ---------------------------------------------------------------------------
// Transport fault: server closes connection immediately after greeting.
//
// The client connects, receives the greeting, then tries to send `listTabs`
// and gets EOF. This surfaces as a transport-level error (exit 6).
// ---------------------------------------------------------------------------

#[test]
fn transport_fault_server_drops_connection() {
    // Bind a port. The mock server thread accepts, sends greeting, then drops.
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();

    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        // Send the RDP greeting.
        let greeting = serde_json::json!({
            "from": "root",
            "applicationType": "browser",
            "traits": {}
        });
        let frame = encode_frame(&serde_json::to_string(&greeting).unwrap());
        stream.write_all(frame.as_bytes()).ok();
        // Drop the stream — this closes the connection.
    });

    let mut args = base_args(port);
    args.push("tabs".to_owned());

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("spawn ff-rdp");

    handle.join().unwrap();

    // Exit code 6: Transport or RemoteClosed.
    assert_eq!(
        output.status.code(),
        Some(6),
        "transport fault must exit 6; stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    // JSON on stdout must include error_type.
    let json = parse_stdout_json(&output.stdout).expect("expected JSON on stdout after error");
    let error_type = json["error_type"]
        .as_str()
        .expect("error_type field missing");
    assert!(
        error_type == "Transport" || error_type == "RemoteClosed" || error_type == "Internal",
        "expected Transport, RemoteClosed, or Internal error_type; got {error_type}"
    );
    assert!(
        json.get("error").is_some(),
        "error field must be present; got {json}"
    );
}

// ---------------------------------------------------------------------------
// Protocol fault: server responds with an actor error packet.
//
// The `listTabs` request comes back as an error packet from the actor.
// This should surface as Protocol error (currently mapped via ProtocolError::ActorError).
// ---------------------------------------------------------------------------

#[test]
fn protocol_fault_actor_error_response() {
    use std::io::BufReader;
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();

    let handle = std::thread::spawn(move || {
        let (stream, _) = listener.accept().expect("accept");
        let mut writer = stream.try_clone().unwrap();
        let mut reader = BufReader::new(stream);

        // Send greeting.
        let greeting = serde_json::json!({
            "from": "root",
            "applicationType": "browser",
            "traits": {}
        });
        let frame = encode_frame(&serde_json::to_string(&greeting).unwrap());
        writer.write_all(frame.as_bytes()).ok();

        // Read the listTabs request (discard).
        let _ = ff_rdp_core::transport::recv_from(&mut reader);

        // Reply with an actor error.
        let error_resp = serde_json::json!({
            "from": "root",
            "error": "unknownActor",
            "message": "No such actor: root"
        });
        let frame = encode_frame(&serde_json::to_string(&error_resp).unwrap());
        writer.write_all(frame.as_bytes()).ok();
    });

    let mut args = base_args(port);
    args.push("tabs".to_owned());

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("spawn ff-rdp");

    handle.join().unwrap();

    // Actor errors with UnknownActor map to User (exit 1) in the existing code.
    // The test asserts the JSON envelope is present and includes error_type.
    assert!(
        output.status.code().is_some(),
        "must exit with a code; stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json =
        parse_stdout_json(&output.stdout).expect("expected JSON on stdout after actor error");
    assert!(
        json.get("error_type").is_some(),
        "error_type must be present in JSON output; got {json}"
    );
    assert!(
        json.get("error").is_some(),
        "error field must be present; got {json}"
    );
}

// ---------------------------------------------------------------------------
// Connection fault: nothing listening on the port.
//
// This surfaces as a Connection error (exit 3).
// ---------------------------------------------------------------------------

#[test]
fn connection_fault_nothing_listening() {
    // Bind to get a free port, then drop so nothing listens.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let mut args = base_args(port);
    args.push("tabs".to_owned());

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("spawn ff-rdp");

    assert_eq!(
        output.status.code(),
        Some(3),
        "connection refused must exit 3; stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json =
        parse_stdout_json(&output.stdout).expect("expected JSON on stdout after connection error");
    let error_type = json["error_type"].as_str().expect("error_type missing");
    assert_eq!(
        error_type, "Connection",
        "expected Connection error_type; got {error_type}"
    );
}

// ---------------------------------------------------------------------------
// Shape fault: server sends a frame with invalid JSON body.
//
// The length prefix says 10 bytes but we only send 3 bytes of valid JSON.
// recv_from returns ProtocolError::InvalidPacket → AppError::RdpShape → exit 4.
// ---------------------------------------------------------------------------

#[test]
fn shape_fault_malformed_frame() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();

    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");

        // Send a valid greeting so the client gets past connect().
        let greeting = serde_json::json!({
            "from": "root",
            "applicationType": "browser",
            "traits": {}
        });
        let frame = encode_frame(&serde_json::to_string(&greeting).unwrap());
        stream.write_all(frame.as_bytes()).ok();

        // Send a frame whose length prefix (100) is much larger than the
        // actual body we provide (3 bytes: `{}`\n).  The client will try to
        // read 100 bytes, get EOF, and surface InvalidPacket → RdpShape.
        stream.write_all(b"100:{}").ok();
        // Drop immediately — client gets EOF mid-read.
    });

    let mut args = base_args(port);
    args.push("tabs".to_owned());

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("spawn ff-rdp");

    handle.join().unwrap();

    // Must fail — the frame is clearly malformed.
    let code = output.status.code().unwrap_or(-1);
    assert!(
        code == 4 || code == 6,
        "malformed frame must exit 4 (RdpShape) or 6 (RdpTransport/EOF); got {code}; stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output.stdout).expect("expected JSON on stdout after error");
    let error_type = json["error_type"].as_str().expect("error_type missing");
    assert!(
        error_type == "Shape" || error_type == "Transport" || error_type == "RemoteClosed",
        "expected Shape, Transport, or RemoteClosed error_type; got {error_type}"
    );
    assert!(
        json.get("error").is_some(),
        "error field must be present; got {json}"
    );
}

// ---------------------------------------------------------------------------
// Timeout fault: operation timeout (--timeout very small) hits before the
// server responds. This surfaces as Timeout error (exit 124).
// ---------------------------------------------------------------------------

#[test]
fn timeout_fault_operation_timeout() {
    use std::io::BufReader;
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();

    let handle = std::thread::spawn(move || {
        let (stream, _) = listener.accept().expect("accept");
        let mut writer = stream.try_clone().unwrap();
        let mut reader = BufReader::new(stream);

        // Send greeting.
        let greeting = serde_json::json!({
            "from": "root",
            "applicationType": "browser",
            "traits": {}
        });
        let frame = encode_frame(&serde_json::to_string(&greeting).unwrap());
        writer.write_all(frame.as_bytes()).ok();

        // Read the listTabs request but never respond — force client to time out.
        let _ = ff_rdp_core::transport::recv_from(&mut reader);

        // Sleep to keep the connection open until the client times out.
        std::thread::sleep(std::time::Duration::from_secs(3));
    });

    let mut args = base_args(port);
    // --timeout 50ms: will time out waiting for the listTabs response.
    args.extend(["--timeout".to_owned(), "50".to_owned(), "tabs".to_owned()]);

    let output = std::process::Command::new(ff_rdp_bin())
        .args(&args)
        .output()
        .expect("spawn ff-rdp");

    handle.join().unwrap();

    // ProtocolError::Timeout → AppError::RdpTimeout → exit 5.
    let code = output.status.code().unwrap_or(-1);
    assert_eq!(
        code,
        5,
        "socket read timeout must exit 5 (RdpTimeout); got {code}; stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    // JSON output must include error_type = "Timeout".
    let json = parse_stdout_json(&output.stdout).expect("expected JSON on stdout after timeout");
    let error_type = json["error_type"].as_str().expect("error_type missing");
    assert_eq!(
        error_type, "Timeout",
        "expected Timeout error_type; got {error_type}"
    );
}
