use std::collections::HashSet;
use std::io::{BufReader, Write as _};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde_json::{Value, json};

use ff_rdp_core::transport::{RdpTransport, encode_frame, recv_from};
use ff_rdp_core::{ProtocolError, RootActor, TabActor, WatcherActor, WebConsoleActor};

use super::buffer::EventBuffer;
use super::registry::{self, DaemonInfo};

/// Resource types the daemon subscribes to at startup.
const WATCHED_RESOURCE_TYPES: &[&str] = &["network-event", "console-message", "error-message"];

/// A streaming subscriber: a connected CLI client that has requested one or
/// more resource types to be forwarded in real time.
struct StreamSubscriber {
    /// Unique client identity token (OS socket handle / file descriptor).
    id: RawHandle,
    /// Write-half of the subscriber's TCP connection.
    writer: TcpStream,
    /// Resource types this subscriber wants to receive.
    types: HashSet<String>,
}

struct SharedState {
    buffer: Mutex<EventBuffer>,
    /// Write-half of the current "RPC" CLI client, if any.
    ///
    /// This is the client that sends Firefox RDP requests (e.g. `eval`) and
    /// needs the corresponding responses forwarded back.  Only one RPC client
    /// can be active at a time (Firefox RDP has no per-request correlation ID
    /// for most messages, so we cannot demultiplex responses to multiple
    /// concurrent senders).  Replaced atomically when a new client connects.
    ///
    /// The `RawHandle` is the identity of the *original* stream (not the
    /// `try_clone`d writer), so disconnect cleanup can reliably compare it
    /// against `client_id` which is taken from the original stream.
    rpc_writer: Mutex<Option<(RawHandle, TcpStream)>>,
    /// All currently-connected streaming subscribers.
    ///
    /// These are clients that have issued one or more `stream` daemon requests
    /// and only need watcher events forwarded — they never send Firefox RDP
    /// requests.  Multiple concurrent streaming subscribers are supported.
    stream_subs: Mutex<Vec<StreamSubscriber>>,
    greeting: Value,
    start_time: Instant,
    last_activity: Mutex<Instant>,
    shutdown: AtomicBool,
    /// The actor ID of the daemon's own watcher subscription.
    ///
    /// Only `resources-available-array` / `resources-updated-array` events
    /// whose `from` field matches this actor are treated as watcher events and
    /// dispatched/buffered.  Events from other watchers (e.g. created by CLI
    /// clients for the cookies or storage command) are forwarded to the RPC
    /// client instead, so that the protocol handshake completes correctly.
    watcher_actor: String,
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

    // Obtain the console actor ID and activate its internal listeners so that
    // console.log() calls from eval (on any connection) are delivered through
    // the watcher's console-message subscription.  Without startListeners the
    // watcher subscription is registered but Firefox does not emit events.
    let target_info =
        TabActor::get_target(&mut transport, &tab_actor).context("getting tab target")?;
    if let Err(e) = WebConsoleActor::start_listeners(
        &mut transport,
        &target_info.console_actor,
        &["PageError", "ConsoleAPI"],
    ) {
        eprintln!("daemon: startListeners failed (non-fatal): {e}");
    }

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
        rpc_writer: Mutex::new(None),
        stream_subs: Mutex::new(Vec::new()),
        greeting,
        start_time: Instant::now(),
        last_activity: Mutex::new(Instant::now()),
        shutdown: AtomicBool::new(false),
        watcher_actor: watcher_actor.as_ref().to_owned(),
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
///   are forwarded to each matching stream subscriber, or buffered when no
///   subscriber is interested in that resource type.
/// - All other messages (RDP request responses) are forwarded to the current
///   RPC client, if any.
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

                if is_watcher_event(&msg, &state.watcher_actor) {
                    // Route to matching streaming subscribers or buffer.
                    dispatch_watcher_event(state, &msg);
                } else if is_console_push_event(&msg) {
                    // Firefox 149+: direct consoleAPICall / pageError push.
                    // Forward to console-message/error-message stream subscribers
                    // AND to the RPC client (e.g. eval may be awaiting results).
                    dispatch_console_push_event(state, &msg);
                    forward_to_rpc_client(&state.rpc_writer, &msg);
                } else {
                    forward_to_rpc_client(&state.rpc_writer, &msg);
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

/// Returns `true` only for `resources-available-array` / `resources-updated-array`
/// events that originate from the **daemon's own watcher actor**.
///
/// Events of the same type from other watcher actors (e.g. one created by a
/// CLI command forwarded through the daemon) must be forwarded to the RPC
/// client so that the `watchResources` handshake completes correctly.
fn is_watcher_event(msg: &Value, daemon_watcher_actor: &str) -> bool {
    let is_watcher_type = matches!(
        msg.get("type").and_then(Value::as_str),
        Some("resources-available-array" | "resources-updated-array")
    );
    if !is_watcher_type {
        return false;
    }
    // Only intercept events sent by the daemon's own watcher.
    msg.get("from").and_then(Value::as_str) == Some(daemon_watcher_actor)
}

/// Return `true` when `msg` is a direct console push notification from the
/// console actor: either `consoleAPICall` (from `console.log()` etc.) or
/// `pageError` (from uncaught JS errors).
///
/// Firefox 149+ delivers these directly to the connection that called
/// `startListeners` rather than routing them through the watcher's
/// `resources-available-array` stream.  The daemon must forward them to
/// stream subscribers registered for `console-message` or `error-message`
/// so that `console --follow` receives them even in daemon mode.
fn is_console_push_event(msg: &Value) -> bool {
    matches!(
        msg.get("type").and_then(Value::as_str),
        Some("consoleAPICall" | "pageError")
    )
}

/// Forward a direct console push event to stream subscribers.
///
/// - `consoleAPICall` is forwarded to subscribers registered for `"console-message"`.
/// - `pageError` is forwarded to subscribers registered for `"error-message"`.
///
/// The raw message is sent as-is; `follow_loop` in the CLI already handles
/// both `consoleAPICall` and `pageError` via `parse_console_notification`.
fn dispatch_console_push_event(state: &SharedState, msg: &Value) {
    let msg_type = msg.get("type").and_then(Value::as_str).unwrap_or_default();
    let target_resource_type = match msg_type {
        "consoleAPICall" => "console-message",
        "pageError" => "error-message",
        _ => return,
    };

    let json = match serde_json::to_string(msg) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("daemon: could not serialise console push event: {e}");
            return;
        }
    };
    let frame = encode_frame(&json);

    // unwrap: poisoned mutex — daemon should crash.
    let mut subs = state.stream_subs.lock().unwrap();
    let mut dead: Vec<usize> = Vec::new();

    for (i, sub) in subs.iter_mut().enumerate() {
        if sub.types.contains(target_resource_type)
            && sub.writer.write_all(frame.as_bytes()).is_err()
        {
            dead.push(i);
        }
    }

    for i in dead.into_iter().rev() {
        subs.remove(i);
    }
}

/// Dispatch a watcher event: forward to each streaming subscriber whose type
/// set overlaps the event's resource types.  Resource types that have no
/// subscriber are buffered for later drain requests.
fn dispatch_watcher_event(state: &SharedState, msg: &Value) {
    let Some(array) = msg.get("array").and_then(Value::as_array) else {
        return;
    };

    // Collect the resource types present in this event.
    let mut event_types: Vec<&str> = Vec::new();
    for sub in array {
        if let Some(sub_arr) = sub.as_array()
            && sub_arr.len() == 2
            && let Some(rt) = sub_arr[0].as_str()
        {
            event_types.push(rt);
        }
    }

    // Determine which types have a subscriber and which need buffering.
    // unwrap: poisoned mutex — daemon should crash.
    let mut subs = state.stream_subs.lock().unwrap();

    // Track which resource types were forwarded to at least one subscriber.
    let mut forwarded_types: HashSet<&str> = HashSet::new();

    // Serialise the message once (shared across all subscribers).
    let json = match serde_json::to_string(msg) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("daemon: could not serialise watcher event: {e}");
            return;
        }
    };
    let frame = encode_frame(&json);

    // Forward to each subscriber that wants at least one type in this event.
    // Collect indices of dead subscribers for removal after iteration.
    let mut dead: Vec<usize> = Vec::new();
    for (i, sub) in subs.iter_mut().enumerate() {
        let wants = event_types.iter().any(|t| sub.types.contains(*t));
        if wants {
            if sub.writer.write_all(frame.as_bytes()).is_err() {
                dead.push(i);
            } else {
                for t in &event_types {
                    if sub.types.contains(*t) {
                        forwarded_types.insert(t);
                    }
                }
            }
        }
    }

    // Remove dead subscribers in reverse order to preserve indices.
    for i in dead.into_iter().rev() {
        subs.remove(i);
    }

    // Drop the lock before acquiring the buffer lock to avoid lock ordering
    // issues.
    drop(subs);

    // Buffer any resource types that were NOT forwarded to any subscriber.
    let unbuffered_types: Vec<&str> = event_types
        .iter()
        .filter(|t| !forwarded_types.contains(*t))
        .copied()
        .collect();

    if !unbuffered_types.is_empty() {
        buffer_watcher_event_for_types(&state.buffer, msg, &unbuffered_types);
    }
}

/// Forward a message to the current RPC client, if one is connected.
///
/// The lock is held for the entire write to prevent interleaved frames from
/// the firefox-reader thread and the client-handler thread.
/// On write error the writer is cleared (treated as disconnected).
fn forward_to_rpc_client(rpc_writer: &Mutex<Option<(RawHandle, TcpStream)>>, msg: &Value) {
    // Serialise first — no lock needed.
    let json = match serde_json::to_string(msg) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("daemon: could not serialise Firefox message: {e}");
            return;
        }
    };
    let frame = encode_frame(&json);

    // unwrap: poisoned mutex means a thread panicked — daemon should crash.
    let mut guard = rpc_writer.lock().unwrap();
    let Some((_id, writer)) = guard.as_mut() else {
        return;
    };
    if writer.write_all(frame.as_bytes()).is_err() {
        // Client disconnected while we were trying to write.
        *guard = None;
    }
}

/// Parse a watcher event and insert individual resource items into the buffer,
/// but only for the listed resource types.
fn buffer_watcher_event_for_types(
    buffer: &Mutex<EventBuffer>,
    msg: &Value,
    types_to_buffer: &[&str],
) {
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
        if !types_to_buffer.contains(&resource_type) {
            continue;
        }
        let Some(items) = sub_arr[1].as_array() else {
            continue;
        };
        for item in items {
            buf.insert(resource_type, item.clone());
        }
    }
}

/// Parse a watcher event and insert ALL resource items into the buffer.
///
/// Used in tests to verify buffering behaviour without subscribers.
#[cfg(test)]
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

// ---------------------------------------------------------------------------
// Main accept loop
// ---------------------------------------------------------------------------

/// Accept CLI connections in a loop, spawning a handler thread per client.
///
/// Returns when:
/// - `state.shutdown` is set (signal or Firefox disconnection), or
/// - the idle timeout fires while no client is connected and the buffer has
///   had no activity.
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
                let state_clone = Arc::clone(state);
                let writer_clone = Arc::clone(firefox_writer);
                thread::Builder::new()
                    .name("cli-client".into())
                    .spawn(move || {
                        if let Err(e) = handle_client(&state_clone, stream, &writer_clone) {
                            eprintln!("daemon: client session error: {e:#}");
                        }
                        *state_clone.last_activity.lock().unwrap() = Instant::now();
                    })
                    .context("spawning client handler thread")?;
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
/// 2. Registers the client as the current RPC forwarding target.
/// 3. Reads client messages in a loop, forwarding them to Firefox or handling
///    daemon-local messages inline.
/// 4. When a `stream` daemon request is received the client is also added to
///    the stream-subscriber list; the client remains in that list until it
///    disconnects or issues a `stop-stream` for all its types.
/// 5. On EOF or error, unregisters the client from all roles and returns.
fn handle_client(
    state: &Arc<SharedState>,
    stream: TcpStream,
    firefox_writer: &Arc<Mutex<TcpStream>>,
) -> Result<()> {
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .context("setting client read timeout")?;
    // Best-effort: disable Nagle for lower latency.
    let _ = stream.set_nodelay(true);

    // Send the cached greeting so the client can identify the connected Firefox.
    let greeting_json = serde_json::to_string(&state.greeting).context("serialising greeting")?;
    let greeting_frame = encode_frame(&greeting_json);

    // Write the greeting before registering the client as the forwarding
    // target — no concurrent writes are possible yet.
    {
        let mut writer = stream
            .try_clone()
            .context("cloning client stream for greeting")?;
        writer
            .write_all(greeting_frame.as_bytes())
            .context("sending greeting to CLI client")?;
    }

    // Capture the client identity from the *original* stream before any
    // try_clone() calls — cloned streams get a different OS handle.
    let client_id = stream.as_raw_fd_or_handle();

    // Register this client as the current RPC forwarding target.
    // The previous RPC client (if any) is simply replaced.
    //
    // KNOWN LIMITATION: When multiple CLI clients connect simultaneously,
    // the last one becomes the RPC writer and may receive RDP responses
    // intended for a previous client.  Firefox RDP lacks per-request
    // correlation IDs, so there is no way to demultiplex responses to
    // the correct client.  This is not a security concern (all clients
    // run as the same local user on localhost) but can cause confusing
    // behaviour when running parallel CLI invocations through the daemon.
    // Workaround: use `--no-daemon` for parallel CLI usage.
    let client_writer = stream
        .try_clone()
        .context("cloning client stream for RPC forwarding")?;
    {
        let mut guard = state.rpc_writer.lock().unwrap();
        *guard = Some((client_id, client_writer));
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
                    // Provide a fresh writer clone for this client so that
                    // handle_daemon_message can register a StreamSubscriber
                    // that writes to the correct connection.
                    let writer_for_sub = reader.get_ref().try_clone().ok();
                    let response = handle_daemon_message(state, &msg, client_id, writer_for_sub);
                    let resp_json =
                        serde_json::to_string(&response).context("serialising daemon response")?;
                    let frame = encode_frame(&resp_json);
                    // Write through the rpc_writer mutex to prevent interleaving
                    // with forward_to_rpc_client on the firefox-reader thread.
                    let mut guard = state.rpc_writer.lock().unwrap();
                    if let Some((_id, w)) = guard.as_mut() {
                        w.write_all(frame.as_bytes())
                            .context("sending daemon response to CLI client")?;
                    }
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
            Err(_) => {
                // EOF or connection reset: client disconnected.
                break;
            }
        }
    }

    // Remove this client from the stream-subscriber list.
    state
        .stream_subs
        .lock()
        .unwrap()
        .retain(|s| s.id != client_id);

    // Unregister this client as RPC target only if it is still the current one
    // (another client may have already taken over).
    {
        let mut guard = state.rpc_writer.lock().unwrap();
        // Compare by the stored identity (taken from the original stream,
        // not from the try_clone'd writer whose OS handle differs).
        if guard.as_ref().is_some_and(|(id, _)| *id == client_id) {
            *guard = None;
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Platform-portable socket handle extraction
// ---------------------------------------------------------------------------

/// A platform-portable type for a raw socket handle / file descriptor.
///
/// Used as a unique client token — it is stable for the lifetime of the
/// `TcpStream` and acts as a cheap address-based identity key.
#[cfg(unix)]
type RawHandle = std::os::unix::io::RawFd;
#[cfg(windows)]
type RawHandle = std::os::windows::io::RawSocket;

trait AsRawFdOrHandle {
    fn as_raw_fd_or_handle(&self) -> RawHandle;
}

#[cfg(unix)]
impl AsRawFdOrHandle for TcpStream {
    fn as_raw_fd_or_handle(&self) -> RawHandle {
        use std::os::unix::io::AsRawFd;
        self.as_raw_fd()
    }
}

#[cfg(windows)]
impl AsRawFdOrHandle for TcpStream {
    fn as_raw_fd_or_handle(&self) -> RawHandle {
        use std::os::windows::io::AsRawSocket;
        self.as_raw_socket()
    }
}

// ---------------------------------------------------------------------------
// Daemon-local message handling
// ---------------------------------------------------------------------------

/// Handle a message addressed `to: "daemon"`.
///
/// `client_id` is the raw handle of the sending client's TCP stream — used to
/// identify which stream-subscriber entry to modify when processing `stream`
/// and `stop-stream` requests.
///
/// `client_writer` is the client's own write-half (a `try_clone` of its
/// original stream), supplied by `handle_client` where the stream is
/// available.  It is used when a new `StreamSubscriber` entry needs to be
/// created so that the subscriber's writer is guaranteed to belong to the
/// correct client, not whatever happens to be stored in `rpc_writer`.
fn handle_daemon_message(
    state: &SharedState,
    msg: &Value,
    client_id: RawHandle,
    client_writer: Option<TcpStream>,
) -> Value {
    let msg_type = msg.get("type").and_then(Value::as_str).unwrap_or_default();

    match msg_type {
        "drain" => {
            let Some(resource_type) = msg
                .get("resourceType")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            else {
                return json!({
                    "from": "daemon",
                    "error": "drain requires a non-empty resourceType field",
                });
            };
            let events = state.buffer.lock().unwrap().drain(resource_type);
            json!({
                "from": "daemon",
                "events": events,
            })
        }
        "stream" => {
            let Some(resource_type) = msg
                .get("resourceType")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            else {
                return json!({
                    "from": "daemon",
                    "error": "stream requires a non-empty resourceType field",
                });
            };
            // Clear existing buffered events for this type so the client
            // only receives events from this point forward.
            let _discarded = state.buffer.lock().unwrap().drain(resource_type);

            // Add this resource type to the client's subscriber entry.
            // If the client is not yet a subscriber, add it now.
            let mut subs = state.stream_subs.lock().unwrap();
            if let Some(sub) = subs.iter_mut().find(|s| s.id == client_id) {
                sub.types.insert(resource_type.to_owned());
            } else if let Some(writer) = client_writer {
                // Create a new subscriber entry using the caller-supplied
                // writer which belongs to this specific client.
                let mut types = HashSet::new();
                types.insert(resource_type.to_owned());
                subs.push(StreamSubscriber {
                    id: client_id,
                    writer,
                    types,
                });
            }

            json!({
                "from": "daemon",
                "streaming": true,
                "resourceType": resource_type,
            })
        }
        "stop-stream" => {
            let Some(resource_type) = msg
                .get("resourceType")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            else {
                return json!({
                    "from": "daemon",
                    "error": "stop-stream requires a non-empty resourceType field",
                });
            };
            let mut subs = state.stream_subs.lock().unwrap();
            if let Some(sub) = subs.iter_mut().find(|s| s.id == client_id) {
                sub.types.remove(resource_type);
            }
            // Remove the subscriber entry if it has no types left.
            subs.retain(|s| !s.types.is_empty());
            json!({
                "from": "daemon",
                "streaming": false,
                "resourceType": resource_type,
            })
        }
        "status" => {
            let uptime = state.start_time.elapsed().as_secs();
            let sizes = state.buffer.lock().unwrap().sizes();
            let subscriber_count = state.stream_subs.lock().unwrap().len();
            json!({
                "from": "daemon",
                "uptime_secs": uptime,
                "buffer_sizes": sizes,
                "stream_subscriber_count": subscriber_count,
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

    // A minimal test-only SharedState with no real sockets.
    fn test_state() -> SharedState {
        SharedState {
            buffer: Mutex::new(EventBuffer::new()),
            rpc_writer: Mutex::new(None),
            stream_subs: Mutex::new(Vec::new()),
            greeting: json!({"applicationType": "browser"}),
            start_time: Instant::now(),
            last_activity: Mutex::new(Instant::now()),
            shutdown: AtomicBool::new(false),
            watcher_actor: String::new(),
        }
    }

    // Sentinel client_id used in tests that do not exercise subscriber logic.
    const TEST_CLIENT_ID: RawHandle = 99;

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
        let resp = handle_daemon_message(&state, &msg, TEST_CLIENT_ID, None);

        assert_eq!(resp["from"], "daemon");
        let events = resp["events"].as_array().expect("events array");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["url"], "https://a.com");
        assert_eq!(events[1]["url"], "https://b.com");

        // Drain again should return empty slice.
        let resp2 = handle_daemon_message(&state, &msg, TEST_CLIENT_ID, None);
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
        let resp = handle_daemon_message(&state, &msg, TEST_CLIENT_ID, None);
        assert_eq!(resp["from"], "daemon");
        assert_eq!(
            resp["events"].as_array().expect("events array").len(),
            0,
            "unknown resource type must yield empty events"
        );
    }

    #[test]
    fn drain_missing_resource_type_returns_error() {
        let state = test_state();
        let msg = json!({"to": "daemon", "type": "drain"});
        let resp = handle_daemon_message(&state, &msg, TEST_CLIENT_ID, None);
        assert!(
            resp["error"].as_str().is_some(),
            "missing resourceType must produce an error"
        );
    }

    #[test]
    fn drain_empty_resource_type_returns_error() {
        let state = test_state();
        let msg = json!({"to": "daemon", "type": "drain", "resourceType": ""});
        let resp = handle_daemon_message(&state, &msg, TEST_CLIENT_ID, None);
        assert!(
            resp["error"].as_str().is_some(),
            "empty resourceType must produce an error"
        );
    }

    #[test]
    fn stream_missing_resource_type_returns_error() {
        let state = test_state();
        let msg = json!({"to": "daemon", "type": "stream"});
        let resp = handle_daemon_message(&state, &msg, TEST_CLIENT_ID, None);
        assert!(
            resp["error"].as_str().is_some(),
            "stream without resourceType must produce an error"
        );
    }

    #[test]
    fn stop_stream_missing_resource_type_returns_error() {
        let state = test_state();
        let msg = json!({"to": "daemon", "type": "stop-stream"});
        let resp = handle_daemon_message(&state, &msg, TEST_CLIENT_ID, None);
        assert!(
            resp["error"].as_str().is_some(),
            "stop-stream without resourceType must produce an error"
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
        let resp = handle_daemon_message(&state, &msg, TEST_CLIENT_ID, None);

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
        let resp = handle_daemon_message(&state, &msg, TEST_CLIENT_ID, None);

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
        let resp = handle_daemon_message(&state, &msg, TEST_CLIENT_ID, None);

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
        let resp = handle_daemon_message(&state, &msg, TEST_CLIENT_ID, None);
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
        let watcher = "server1.conn0.watcher1";
        assert!(
            is_watcher_event(
                &json!({"type": "resources-available-array", "from": watcher}),
                watcher
            ),
            "resources-available-array from the daemon watcher must be recognised"
        );
        assert!(
            is_watcher_event(
                &json!({"type": "resources-updated-array", "from": watcher}),
                watcher
            ),
            "resources-updated-array from the daemon watcher must be recognised"
        );
    }

    #[test]
    fn is_watcher_event_rejects_non_resource_types() {
        let watcher = "server1.conn0.watcher1";
        assert!(
            !is_watcher_event(&json!({"type": "someOtherType", "from": watcher}), watcher),
            "unrelated type must not be a watcher event"
        );
        assert!(
            !is_watcher_event(&json!({"from": watcher}), watcher),
            "message without type must not be a watcher event"
        );
        assert!(
            !is_watcher_event(&json!({}), watcher),
            "empty message must not be a watcher event"
        );
    }

    #[test]
    fn is_watcher_event_rejects_events_from_other_watchers() {
        // Events from a watcher the CLI created (not the daemon's watcher)
        // must NOT be intercepted — they need to reach the RPC client.
        let daemon_watcher = "server1.conn0.watcher1";
        let cli_watcher = "server1.conn0.watcher99";
        assert!(
            !is_watcher_event(
                &json!({"type": "resources-available-array", "from": cli_watcher}),
                daemon_watcher
            ),
            "resources-available-array from a non-daemon watcher must not be intercepted"
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

    // -----------------------------------------------------------------------
    // handle_daemon_message — stream / stop-stream
    // -----------------------------------------------------------------------

    #[test]
    fn stream_clears_buffer_and_returns_streaming_ack() {
        let state = test_state();
        // Pre-populate buffer.
        state
            .buffer
            .lock()
            .expect("buffer lock")
            .insert("network-event", json!({"url": "https://stale.com"}));

        let msg = json!({"to": "daemon", "type": "stream", "resourceType": "network-event"});
        let resp = handle_daemon_message(&state, &msg, TEST_CLIENT_ID, None);

        assert_eq!(resp["from"], "daemon");
        assert_eq!(resp["streaming"], true);
        assert_eq!(resp["resourceType"], "network-event");

        // Buffer must be cleared.
        assert!(
            state
                .buffer
                .lock()
                .expect("buffer lock")
                .drain("network-event")
                .is_empty(),
            "buffer must be empty after stream request"
        );
    }

    #[test]
    fn stop_stream_returns_streaming_false() {
        let state = test_state();
        let msg = json!({"to": "daemon", "type": "stop-stream", "resourceType": "network-event"});
        let resp = handle_daemon_message(&state, &msg, TEST_CLIENT_ID, None);

        assert_eq!(resp["from"], "daemon");
        assert_eq!(resp["streaming"], false);
    }

    // -----------------------------------------------------------------------
    // buffer_watcher_event_for_types
    // -----------------------------------------------------------------------

    #[test]
    fn buffer_for_types_only_buffers_matching_types() {
        let state = test_state();
        let msg = json!({
            "type": "resources-available-array",
            "array": [
                ["network-event", [{"actor": "a1", "url": "https://x.com"}]],
                ["console-message", [{"msg": "hello"}]]
            ]
        });
        // Only buffer network-event.
        buffer_watcher_event_for_types(&state.buffer, &msg, &["network-event"]);

        let mut buf = state.buffer.lock().expect("buffer lock");
        let net = buf.drain("network-event");
        assert_eq!(net.len(), 1);
        let console = buf.drain("console-message");
        assert!(console.is_empty(), "console-message must not be buffered");
    }

    // -----------------------------------------------------------------------
    // is_watcher_event (duplicate guard)
    // -----------------------------------------------------------------------

    #[test]
    fn should_stream_event_returns_true_for_streaming_type() {
        // Verify dispatch_watcher_event routing logic via buffer fallback:
        // if no subscriber claims a type, it must land in the buffer.
        let state = test_state();
        let msg = json!({
            "type": "resources-available-array",
            "array": [["network-event", [{"actor": "a1"}]]]
        });
        dispatch_watcher_event(&state, &msg);
        // No subscriber registered → falls into buffer.
        let events = state.buffer.lock().unwrap().drain("network-event");
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn dispatch_buffers_when_no_subscribers() {
        let state = test_state();
        let msg = json!({
            "type": "resources-available-array",
            "array": [["console-message", [{"msg": "hi"}]]]
        });
        dispatch_watcher_event(&state, &msg);
        let events = state.buffer.lock().unwrap().drain("console-message");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["msg"], "hi");
    }

    // -----------------------------------------------------------------------
    // is_console_push_event
    // -----------------------------------------------------------------------

    #[test]
    fn is_console_push_event_detects_console_api_call() {
        assert!(
            is_console_push_event(&json!({"type": "consoleAPICall", "message": {}})),
            "consoleAPICall must be a console push event"
        );
    }

    #[test]
    fn is_console_push_event_detects_page_error() {
        assert!(
            is_console_push_event(&json!({"type": "pageError", "pageError": {}})),
            "pageError must be a console push event"
        );
    }

    #[test]
    fn is_console_push_event_rejects_watcher_events() {
        assert!(
            !is_console_push_event(&json!({"type": "resources-available-array"})),
            "resources-available-array must not be a console push event"
        );
        assert!(
            !is_console_push_event(&json!({"type": "evaluationResult"})),
            "evaluationResult must not be a console push event"
        );
        assert!(
            !is_console_push_event(&json!({})),
            "empty message must not be a console push event"
        );
    }

    // -----------------------------------------------------------------------
    // dispatch_console_push_event — uses loopback TCP to verify delivery
    // -----------------------------------------------------------------------

    /// Build a loopback (server, client) TCP pair for use in tests.
    fn loopback_pair() -> (TcpStream, TcpStream) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("local_addr");
        let client = TcpStream::connect(addr).expect("connect");
        let (server, _) = listener.accept().expect("accept");
        (server, client)
    }

    #[test]
    fn dispatch_console_push_forwards_console_api_call_to_console_message_subscriber() {
        use std::io::Read;

        let state = test_state();
        let (server_side, mut client_side) = loopback_pair();

        // Register a stream subscriber for "console-message".
        state.stream_subs.lock().unwrap().push(StreamSubscriber {
            id: 1,
            writer: server_side,
            types: {
                let mut s = HashSet::new();
                s.insert("console-message".to_owned());
                s
            },
        });

        let msg = json!({
            "type": "consoleAPICall",
            "from": "server1.conn0.child0/consoleActor0",
            "message": {
                "arguments": ["hello"],
                "level": "log",
                "filename": "debugger eval code",
                "lineNumber": 1,
                "columnNumber": 9,
                "timeStamp": 1_234_567_890.0
            }
        });

        dispatch_console_push_event(&state, &msg);

        // The subscriber's writer should have received the framed message.
        client_side
            .set_read_timeout(Some(Duration::from_millis(500)))
            .unwrap();
        let mut buf = Vec::new();
        let _ = client_side.read_to_end(&mut buf);
        let raw = String::from_utf8_lossy(&buf);

        assert!(
            raw.contains("consoleAPICall"),
            "forwarded frame must contain consoleAPICall; got: {raw}"
        );
        assert!(
            raw.contains("hello"),
            "forwarded frame must contain message content; got: {raw}"
        );
    }

    #[test]
    fn dispatch_console_push_forwards_page_error_to_error_message_subscriber() {
        use std::io::Read;

        let state = test_state();
        let (server_side, mut client_side) = loopback_pair();

        // Register a stream subscriber for "error-message".
        state.stream_subs.lock().unwrap().push(StreamSubscriber {
            id: 2,
            writer: server_side,
            types: {
                let mut s = HashSet::new();
                s.insert("error-message".to_owned());
                s
            },
        });

        let msg = json!({
            "type": "pageError",
            "from": "server1.conn0.child0/consoleActor0",
            "pageError": {
                "errorMessage": "ReferenceError: foo is not defined",
                "sourceName": "https://example.com/app.js",
                "lineNumber": 10,
                "columnNumber": 3,
                "timeStamp": 1_234_567_890.0
            }
        });

        dispatch_console_push_event(&state, &msg);

        client_side
            .set_read_timeout(Some(Duration::from_millis(500)))
            .unwrap();
        let mut buf = Vec::new();
        let _ = client_side.read_to_end(&mut buf);
        let raw = String::from_utf8_lossy(&buf);

        assert!(
            raw.contains("pageError"),
            "forwarded frame must contain pageError; got: {raw}"
        );
        assert!(
            raw.contains("ReferenceError"),
            "forwarded frame must contain error message; got: {raw}"
        );
    }

    #[test]
    fn dispatch_console_push_does_not_deliver_to_wrong_subscriber_type() {
        use std::io::Read;

        let state = test_state();
        let (server_side, mut client_side) = loopback_pair();

        // Register subscriber for "network-event" only — NOT console-message.
        state.stream_subs.lock().unwrap().push(StreamSubscriber {
            id: 3,
            writer: server_side,
            types: {
                let mut s = HashSet::new();
                s.insert("network-event".to_owned());
                s
            },
        });

        let msg = json!({
            "type": "consoleAPICall",
            "message": {"arguments": ["secret"], "level": "log", "timeStamp": 1.0}
        });

        dispatch_console_push_event(&state, &msg);

        // The writer is not closed; read must time out with no data.
        client_side
            .set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        let mut buf = vec![0u8; 256];
        let result = client_side.read(&mut buf);
        assert!(
            result.is_err() || result.is_ok_and(|n| n == 0),
            "network-event subscriber must not receive consoleAPICall"
        );
    }
}
