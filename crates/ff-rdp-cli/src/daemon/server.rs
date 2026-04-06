use std::io::{BufReader, Write as _};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde_json::{Value, json};

use ff_rdp_core::transport::{RdpTransport, encode_frame, recv_from};
use ff_rdp_core::{ProtocolError, RootActor, TabActor, WatcherActor};

use super::buffer::EventBuffer;
use super::registry::{self, DaemonInfo};

/// Resource types the daemon subscribes to at startup.
const WATCHED_RESOURCE_TYPES: &[&str] = &["network-event", "console-message", "error-message"];

struct SharedState {
    buffer: Mutex<EventBuffer>,
    /// Write half of the current CLI client's TCP connection, if any.
    cli_writer: Mutex<Option<TcpStream>>,
    greeting: Value,
    start_time: Instant,
    last_activity: Mutex<Instant>,
    shutdown: AtomicBool,
}

/// Main entry point for the daemon process.
///
/// Runs as `ff-rdp _daemon` and blocks until the idle timeout fires, a fatal
/// Firefox error occurs, or a shutdown signal is received.
pub(crate) fn run_daemon(
    firefox_host: &str,
    firefox_port: u16,
    idle_timeout_secs: u64,
) -> Result<()> {
    let idle_timeout = Duration::from_secs(idle_timeout_secs);
    let connect_timeout = Duration::from_secs(10);

    // Connect to Firefox and perform initial protocol setup.
    let mut transport = RdpTransport::connect_raw(firefox_host, firefox_port, connect_timeout)
        .context("connecting to Firefox")?;
    let greeting = transport.recv().context("reading Firefox greeting")?;
    validate_greeting(&greeting)?;

    let tabs = RootActor::list_tabs(&mut transport).context("listing tabs")?;
    let tab_actor = tabs.first().context("no tabs available")?.actor.clone();
    let watcher_actor =
        TabActor::get_watcher(&mut transport, &tab_actor).context("getting watcher actor")?;
    WatcherActor::watch_resources(&mut transport, &watcher_actor, WATCHED_RESOURCE_TYPES)
        .context("subscribing to resources")?;

    // Listen on a random loopback port; the OS assigns the port number.
    let listener = TcpListener::bind("127.0.0.1:0").context("binding TCP listener")?;
    let proxy_port = listener.local_addr()?.port();
    listener
        .set_nonblocking(true)
        .context("setting listener non-blocking")?;

    // Publish the port so CLI clients can find us.
    let info = DaemonInfo {
        pid: std::process::id(),
        proxy_port,
        firefox_host: firefox_host.to_owned(),
        firefox_port,
        started_at: chrono::Utc::now().to_rfc3339(),
    };
    registry::write_registry(&info).context("writing registry")?;
    eprintln!("daemon: listening on port {proxy_port}, PID {}", info.pid);

    // Split the transport so the reader and writer can live on separate threads.
    let (firefox_reader, firefox_writer) = transport.into_parts();

    let state = Arc::new(SharedState {
        buffer: Mutex::new(EventBuffer::new()),
        cli_writer: Mutex::new(None),
        greeting,
        start_time: Instant::now(),
        last_activity: Mutex::new(Instant::now()),
        shutdown: AtomicBool::new(false),
    });

    setup_signal_handler(&state);

    // The Firefox writer is shared: the main thread may forward CLI messages to
    // Firefox while the reader thread owns the read half exclusively.
    let firefox_writer = Arc::new(Mutex::new(firefox_writer));

    // Spawn the Firefox reader thread.
    let state_for_reader = Arc::clone(&state);
    thread::Builder::new()
        .name("firefox-reader".into())
        .spawn(move || firefox_reader_loop(&state_for_reader, firefox_reader))
        .context("spawning Firefox reader thread")?;

    let result = accept_loop(&state, &listener, &firefox_writer, idle_timeout);

    state.shutdown.store(true, Ordering::Relaxed);
    let _ = registry::remove_registry();
    eprintln!("daemon: shut down");

    result
}

fn validate_greeting(greeting: &Value) -> Result<()> {
    let app_type = greeting
        .get("applicationType")
        .and_then(Value::as_str)
        .unwrap_or("");
    anyhow::ensure!(
        app_type == "browser",
        "unexpected Firefox applicationType: {app_type:?}"
    );
    Ok(())
}

/// Install platform-native signal handlers that set `state.shutdown`.
///
/// On Unix we redirect SIGTERM and SIGINT.  On Windows there is no direct
/// equivalent; the process will be terminated by the OS when the parent
/// exits, so this is intentionally a no-op there.
#[allow(unused_variables)]
fn setup_signal_handler(state: &Arc<SharedState>) {
    // signal-hook requires an additional dependency we want to avoid.
    // For SIGINT the Rust runtime's default handler terminates the process,
    // which skips the explicit cleanup in run_daemon.  SIGTERM similarly
    // terminates immediately.  This is acceptable for now: the registry file
    // is a best-effort hint and stale entries are already handled by the
    // caller (see find_running_daemon in client.rs which checks PID liveness).
    //
    // If a more graceful shutdown is needed in the future, add signal-hook,
    // set state.shutdown here, and rely on run_daemon calling remove_registry
    // after accept_loop returns.
}

// ---------------------------------------------------------------------------
// Firefox reader thread
// ---------------------------------------------------------------------------

/// Read from Firefox indefinitely.
///
/// - Watcher events (`resources-available-array`, `resources-updated-array`)
///   are parsed and stored in the shared buffer.
/// - All other messages are forwarded to the connected CLI client, if any.
/// - A 1-second read timeout lets us check `state.shutdown` periodically.
fn firefox_reader_loop(state: &Arc<SharedState>, mut reader: BufReader<TcpStream>) {
    // Apply a short read timeout so we can check the shutdown flag.
    // The timeout is set on the underlying stream; errors from it are
    // converted to `ProtocolError::Timeout` by `recv_from`.
    if let Err(e) = reader
        .get_ref()
        .set_read_timeout(Some(Duration::from_secs(1)))
    {
        eprintln!("daemon: could not set Firefox read timeout: {e}");
    }

    loop {
        if state.shutdown.load(Ordering::Relaxed) {
            break;
        }

        match recv_from(&mut reader) {
            Ok(msg) => {
                // unwrap: poisoned mutex means a thread panicked — the daemon
                // is in an inconsistent state and crashing is the right action.
                *state.last_activity.lock().unwrap() = Instant::now();

                if is_watcher_event(&msg) {
                    buffer_watcher_event(&state.buffer, &msg);
                } else {
                    forward_to_cli(&state.cli_writer, &msg);
                }
            }
            Err(ProtocolError::Timeout) => {
                // Expected — just loop and re-check the shutdown flag.
            }
            Err(e) => {
                eprintln!("daemon: Firefox connection lost: {e}");
                state.shutdown.store(true, Ordering::Relaxed);
                break;
            }
        }
    }
}

fn is_watcher_event(msg: &Value) -> bool {
    matches!(
        msg.get("type").and_then(Value::as_str),
        Some("resources-available-array" | "resources-updated-array")
    )
}

/// Parse a watcher event and insert individual resource items into the buffer.
///
/// The `array` field contains `[[resource_type, [item, ...]], ...]` pairs.
fn buffer_watcher_event(buffer: &Mutex<EventBuffer>, msg: &Value) {
    let Some(array) = msg.get("array").and_then(Value::as_array) else {
        return;
    };

    // unwrap: poisoned mutex means a thread panicked — crash is intentional.
    let mut buf = buffer.lock().unwrap();
    for sub in array {
        let Some(sub_arr) = sub.as_array() else {
            continue;
        };
        if sub_arr.len() != 2 {
            continue;
        }
        let Some(resource_type) = sub_arr[0].as_str() else {
            continue;
        };
        let Some(items) = sub_arr[1].as_array() else {
            continue;
        };
        for item in items {
            buf.insert(resource_type, item.clone());
        }
    }
}

/// Forward a message to the CLI client if one is currently connected.
///
/// The lock is held only long enough to clone the writer, then released before
/// the I/O call so the mutex is not held across a potentially-blocking write.
/// On write error the writer is cleared (treated as disconnected).
fn forward_to_cli(cli_writer: &Mutex<Option<TcpStream>>, msg: &Value) {
    // Serialise first — no lock needed.
    let json = match serde_json::to_string(msg) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("daemon: could not serialise Firefox message: {e}");
            return;
        }
    };
    let frame = encode_frame(&json);

    // Hold the lock only to clone the stream handle, then release it.
    // `TcpStream::try_clone` duplicates the fd/handle; the write below
    // operates on the clone without holding the mutex.
    let mut writer = {
        // unwrap: poisoned mutex means a thread panicked — daemon should crash.
        let guard = cli_writer.lock().unwrap();
        match guard.as_ref() {
            Some(w) => match w.try_clone() {
                Ok(cloned) => cloned,
                Err(e) => {
                    eprintln!("daemon: could not clone CLI writer: {e}");
                    return;
                }
            },
            None => return,
        }
    };

    if writer.write_all(frame.as_bytes()).is_err() {
        // Client disconnected while we were trying to write.
        // unwrap: same rationale as above.
        *cli_writer.lock().unwrap() = None;
    }
}

// ---------------------------------------------------------------------------
// Main accept loop
// ---------------------------------------------------------------------------

/// Accept CLI connections in a loop.
///
/// Returns when:
/// - `state.shutdown` is set (signal or Firefox disconnection), or
/// - the idle timeout fires while no client is connected.
fn accept_loop(
    state: &Arc<SharedState>,
    listener: &TcpListener,
    firefox_writer: &Arc<Mutex<TcpStream>>,
    idle_timeout: Duration,
) -> Result<()> {
    loop {
        if state.shutdown.load(Ordering::Relaxed) {
            return Ok(());
        }

        // Idle timeout: checked when no client is connected.
        {
            let last = *state.last_activity.lock().unwrap();
            if last.elapsed() > idle_timeout {
                eprintln!("daemon: idle timeout ({idle_timeout:?}), shutting down");
                return Ok(());
            }
        }

        match listener.accept() {
            Ok((stream, _addr)) => {
                *state.last_activity.lock().unwrap() = Instant::now();
                if let Err(e) = handle_client(state, stream, firefox_writer) {
                    eprintln!("daemon: client session error: {e:#}");
                }
                *state.last_activity.lock().unwrap() = Instant::now();
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(100));
            }
            Err(e) => {
                return Err(e).context("accepting CLI client connection");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Per-client handling
// ---------------------------------------------------------------------------

/// Handle a single CLI client connection.
///
/// 1. Sends the cached Firefox greeting.
/// 2. Registers the client as the current forwarding target for Firefox events.
/// 3. Reads client messages in a loop, forwarding them to Firefox or handling
///    daemon-local messages inline.
/// 4. On EOF or error, unregisters the client and returns.
fn handle_client(
    state: &Arc<SharedState>,
    stream: TcpStream,
    firefox_writer: &Arc<Mutex<TcpStream>>,
) -> Result<()> {
    stream
        .set_read_timeout(Some(Duration::from_millis(500)))
        .context("setting client read timeout")?;
    // Best-effort: disable Nagle for lower latency.
    let _ = stream.set_nodelay(true);

    // Send the cached greeting so the client can identify the connected Firefox.
    let greeting_json = serde_json::to_string(&state.greeting).context("serialising greeting")?;
    let greeting_frame = encode_frame(&greeting_json);

    // Clone for writing; `reader` will wrap the original.
    let mut writer = stream
        .try_clone()
        .context("cloning client stream for writer")?;
    writer
        .write_all(greeting_frame.as_bytes())
        .context("sending greeting to CLI client")?;

    // Register this client as the current forwarding target.
    {
        let mut guard = state.cli_writer.lock().unwrap();
        *guard = Some(
            stream
                .try_clone()
                .context("cloning client stream for forwarding")?,
        );
    }

    let mut reader = BufReader::new(stream);

    loop {
        if state.shutdown.load(Ordering::Relaxed) {
            break;
        }

        match recv_from(&mut reader) {
            Ok(msg) => {
                *state.last_activity.lock().unwrap() = Instant::now();

                let to = msg.get("to").and_then(Value::as_str).unwrap_or_default();
                if to == "daemon" {
                    let response = handle_daemon_message(state, &msg);
                    let resp_json =
                        serde_json::to_string(&response).context("serialising daemon response")?;
                    let frame = encode_frame(&resp_json);
                    writer
                        .write_all(frame.as_bytes())
                        .context("sending daemon response to CLI client")?;
                } else {
                    // Forward to Firefox.
                    let json = serde_json::to_string(&msg).context("serialising CLI message")?;
                    let frame = encode_frame(&json);
                    firefox_writer
                        .lock()
                        .unwrap()
                        .write_all(frame.as_bytes())
                        .context("forwarding CLI message to Firefox")?;
                }
            }
            Err(ProtocolError::Timeout) => {
                // Expected poll timeout — re-check shutdown and continue.
            }
            Err(e) => {
                // EOF or connection reset: client disconnected.
                eprintln!("daemon: client read ended: {e}");
                break;
            }
        }
    }

    // Unregister the forwarding target.
    *state.cli_writer.lock().unwrap() = None;

    Ok(())
}

// ---------------------------------------------------------------------------
// Daemon-local message handling
// ---------------------------------------------------------------------------

/// Handle a message addressed `to: "daemon"`.
fn handle_daemon_message(state: &SharedState, msg: &Value) -> Value {
    let msg_type = msg.get("type").and_then(Value::as_str).unwrap_or_default();

    match msg_type {
        "drain" => {
            let resource_type = msg
                .get("resourceType")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let events = state.buffer.lock().unwrap().drain(resource_type);
            json!({
                "from": "daemon",
                "events": events,
            })
        }
        "status" => {
            let uptime = state.start_time.elapsed().as_secs();
            let sizes = state.buffer.lock().unwrap().sizes();
            json!({
                "from": "daemon",
                "uptime_secs": uptime,
                "buffer_sizes": sizes,
            })
        }
        other => {
            json!({
                "from": "daemon",
                "error": format!("unknown daemon message type: {other:?}"),
            })
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::sync::atomic::AtomicBool;
    use std::time::Instant;

    use serde_json::json;

    use super::*;

    fn test_state() -> SharedState {
        SharedState {
            buffer: Mutex::new(EventBuffer::new()),
            cli_writer: Mutex::new(None),
            greeting: json!({"applicationType": "browser"}),
            start_time: Instant::now(),
            last_activity: Mutex::new(Instant::now()),
            shutdown: AtomicBool::new(false),
        }
    }

    // -----------------------------------------------------------------------
    // handle_daemon_message — drain
    // -----------------------------------------------------------------------

    #[test]
    fn drain_returns_buffered_events_and_clears() {
        let state = test_state();
        state
            .buffer
            .lock()
            .expect("buffer lock")
            .insert("network-event", json!({"url": "https://a.com"}));
        state
            .buffer
            .lock()
            .expect("buffer lock")
            .insert("network-event", json!({"url": "https://b.com"}));

        let msg = json!({"to": "daemon", "type": "drain", "resourceType": "network-event"});
        let resp = handle_daemon_message(&state, &msg);

        assert_eq!(resp["from"], "daemon");
        let events = resp["events"].as_array().expect("events array");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["url"], "https://a.com");
        assert_eq!(events[1]["url"], "https://b.com");

        // Drain again should return empty slice.
        let resp2 = handle_daemon_message(&state, &msg);
        assert_eq!(
            resp2["events"]
                .as_array()
                .expect("events array on second drain")
                .len(),
            0
        );
    }

    #[test]
    fn drain_unknown_resource_type_returns_empty() {
        let state = test_state();
        let msg = json!({"to": "daemon", "type": "drain", "resourceType": "nonexistent"});
        let resp = handle_daemon_message(&state, &msg);
        assert_eq!(resp["from"], "daemon");
        assert_eq!(
            resp["events"].as_array().expect("events array").len(),
            0,
            "unknown resource type must yield empty events"
        );
    }

    #[test]
    fn drain_missing_resource_type_returns_empty() {
        let state = test_state();
        // No "resourceType" key at all — defaults to empty string, maps to
        // an unknown bucket.
        let msg = json!({"to": "daemon", "type": "drain"});
        let resp = handle_daemon_message(&state, &msg);
        assert_eq!(
            resp["events"].as_array().expect("events array").len(),
            0,
            "missing resourceType key must yield empty events"
        );
    }

    // -----------------------------------------------------------------------
    // handle_daemon_message — status
    // -----------------------------------------------------------------------

    #[test]
    fn status_returns_uptime_and_buffer_sizes() {
        let state = test_state();
        state
            .buffer
            .lock()
            .expect("buffer lock")
            .insert("network-event", json!({}));
        state
            .buffer
            .lock()
            .expect("buffer lock")
            .insert("console-message", json!({}));
        state
            .buffer
            .lock()
            .expect("buffer lock")
            .insert("console-message", json!({}));

        let msg = json!({"to": "daemon", "type": "status"});
        let resp = handle_daemon_message(&state, &msg);

        assert_eq!(resp["from"], "daemon");
        assert!(
            resp["uptime_secs"].as_u64().is_some(),
            "uptime_secs must be a non-negative integer"
        );
        assert_eq!(
            resp["buffer_sizes"]["network-event"], 1,
            "network-event bucket size mismatch"
        );
        assert_eq!(
            resp["buffer_sizes"]["console-message"], 2,
            "console-message bucket size mismatch"
        );
    }

    #[test]
    fn status_with_empty_buffer_omits_zero_sizes() {
        let state = test_state();
        let msg = json!({"to": "daemon", "type": "status"});
        let resp = handle_daemon_message(&state, &msg);

        assert_eq!(resp["from"], "daemon");
        // sizes() filters out empty buckets, so buffer_sizes should be an
        // empty object (not absent).
        assert!(
            resp["buffer_sizes"].is_object(),
            "buffer_sizes must be a JSON object"
        );
        assert_eq!(
            resp["buffer_sizes"]
                .as_object()
                .expect("buffer_sizes object")
                .len(),
            0,
            "empty buffer must produce zero-entry buffer_sizes"
        );
    }

    // -----------------------------------------------------------------------
    // handle_daemon_message — unknown type
    // -----------------------------------------------------------------------

    #[test]
    fn unknown_message_type_returns_error() {
        let state = test_state();
        let msg = json!({"to": "daemon", "type": "unknown-stuff"});
        let resp = handle_daemon_message(&state, &msg);

        assert_eq!(resp["from"], "daemon");
        let err = resp["error"].as_str().expect("error string");
        assert!(
            err.contains("unknown"),
            "error message must mention 'unknown'; got: {err:?}"
        );
    }

    #[test]
    fn missing_type_field_returns_error() {
        let state = test_state();
        // No "type" key — defaults to empty string, which is unrecognised.
        let msg = json!({"to": "daemon"});
        let resp = handle_daemon_message(&state, &msg);
        assert!(
            resp["error"].as_str().is_some(),
            "missing type must produce an error field"
        );
    }

    // -----------------------------------------------------------------------
    // is_watcher_event
    // -----------------------------------------------------------------------

    #[test]
    fn is_watcher_event_detects_resource_array_types() {
        assert!(
            is_watcher_event(&json!({"type": "resources-available-array"})),
            "resources-available-array must be recognised"
        );
        assert!(
            is_watcher_event(&json!({"type": "resources-updated-array"})),
            "resources-updated-array must be recognised"
        );
    }

    #[test]
    fn is_watcher_event_rejects_non_resource_types() {
        assert!(
            !is_watcher_event(&json!({"type": "someOtherType"})),
            "unrelated type must not be a watcher event"
        );
        assert!(
            !is_watcher_event(&json!({"from": "root"})),
            "message without type must not be a watcher event"
        );
        assert!(
            !is_watcher_event(&json!({})),
            "empty message must not be a watcher event"
        );
    }

    // -----------------------------------------------------------------------
    // buffer_watcher_event
    // -----------------------------------------------------------------------

    #[test]
    fn buffer_watcher_event_stores_items_by_resource_type() {
        let state = test_state();
        let msg = json!({
            "type": "resources-available-array",
            "array": [
                ["network-event", [{"actor": "a1", "url": "https://x.com"}]],
                ["console-message", [{"msg": "hello"}, {"msg": "world"}]]
            ]
        });
        buffer_watcher_event(&state.buffer, &msg);

        let mut buf = state.buffer.lock().expect("buffer lock");
        let net = buf.drain("network-event");
        assert_eq!(net.len(), 1, "expected 1 network-event");
        assert_eq!(net[0]["url"], "https://x.com");

        let console = buf.drain("console-message");
        assert_eq!(console.len(), 2, "expected 2 console-messages");
        assert_eq!(console[0]["msg"], "hello");
        assert_eq!(console[1]["msg"], "world");
    }

    #[test]
    fn buffer_watcher_event_ignores_missing_array_field() {
        let state = test_state();
        // Message without "array" field — must not panic or insert anything.
        let msg = json!({"type": "resources-available-array"});
        buffer_watcher_event(&state.buffer, &msg);
        assert!(
            state.buffer.lock().expect("buffer lock").is_empty(),
            "buffer must remain empty when 'array' field is absent"
        );
    }

    #[test]
    fn buffer_watcher_event_skips_malformed_sub_entries() {
        let state = test_state();
        let msg = json!({
            "type": "resources-available-array",
            "array": [
                // Too short — only one element instead of two.
                ["network-event"],
                // Correct entry mixed in.
                ["console-message", [{"msg": "ok"}]]
            ]
        });
        buffer_watcher_event(&state.buffer, &msg);

        let mut buf = state.buffer.lock().expect("buffer lock");
        // The malformed entry must be silently skipped.
        assert!(
            buf.drain("network-event").is_empty(),
            "malformed entry must produce no events"
        );
        assert_eq!(
            buf.drain("console-message").len(),
            1,
            "valid entry after malformed one must still be stored"
        );
    }

    #[test]
    fn buffer_watcher_event_handles_empty_items_list() {
        let state = test_state();
        let msg = json!({
            "type": "resources-available-array",
            "array": [
                ["network-event", []]
            ]
        });
        buffer_watcher_event(&state.buffer, &msg);
        assert!(
            state.buffer.lock().expect("buffer lock").is_empty(),
            "empty items list must not add any events"
        );
    }
}
