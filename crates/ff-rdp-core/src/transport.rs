use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Sender;
use std::time::Duration;

use serde_json::Value;

use crate::error::{ActorErrorKind, ProtocolError};

// ---------------------------------------------------------------------------
// Payload redactor
// ---------------------------------------------------------------------------

/// Keys whose string values are always redacted regardless of length.
const SENSITIVE_KEYS: &[&str] = &[
    "cookie",
    "set-cookie",
    "authorization",
    "auth-token",
    "x-auth-token",
    "password",
];

/// Keys whose values contain JS source or request body text and should be
/// redacted to avoid leaking eval payloads in traces.
const SOURCE_KEYS: &[&str] = &["text", "expression"];

/// Default maximum string length (in bytes) allowed through the redactor for
/// ad-hoc string values that aren't explicitly listed in `SENSITIVE_KEYS` or
/// `SOURCE_KEYS`.
///
/// Long URLs, query strings, and non-sensitive payload fragments commonly
/// exceed the legacy 32-byte threshold; 256 keeps traces readable while still
/// truncating runaway blobs.  Override at runtime with
/// [`set_redact_threshold`].
pub const DEFAULT_REDACT_THRESHOLD: usize = 256;

/// Runtime-configurable redaction threshold.  `0` means "unset, use the
/// [`DEFAULT_REDACT_THRESHOLD`]".  See [`set_redact_threshold`] /
/// [`redact_threshold`].
static REDACT_THRESHOLD: AtomicUsize = AtomicUsize::new(0);

/// Set the redactor's threshold for un-keyed long strings.
///
/// Sensitive-keyed values (`cookie`, `authorization`, `text`, etc.) are still
/// redacted unconditionally — the threshold only affects the
/// "long-string-anywhere-in-the-tree" rule.
///
/// `bytes = 0` resets to [`DEFAULT_REDACT_THRESHOLD`].
pub fn set_redact_threshold(bytes: usize) {
    REDACT_THRESHOLD.store(bytes, Ordering::Relaxed);
}

/// Current redaction threshold in bytes (default
/// [`DEFAULT_REDACT_THRESHOLD`] when [`set_redact_threshold`] was not called).
pub fn redact_threshold() -> usize {
    let v = REDACT_THRESHOLD.load(Ordering::Relaxed);
    if v == 0 { DEFAULT_REDACT_THRESHOLD } else { v }
}

/// Redact a JSON value and return a redacted clone for safe trace output.
///
/// - All values of keys matching [`SENSITIVE_KEYS`] are replaced.
/// - All values of keys matching [`SOURCE_KEYS`] are replaced.
/// - String values exceeding the [`redact_threshold`] anywhere in the tree
///   are replaced.
///
/// When the `FF_RDP_TRACE_RAW=1` environment variable is set, redaction is
/// skipped and the value is returned as a clone.  This allows local debugging
/// without recompiling.  The env var is read once and cached in a
/// [`std::sync::OnceLock`].
pub fn redact(value: &Value) -> Value {
    if trace_raw_enabled() {
        return value.clone();
    }
    redact_inner(value)
}

/// Returns `true` if raw (un-redacted) trace output is enabled.
///
/// In production the result is cached after the first call via a
/// [`std::sync::OnceLock`].  In tests, [`set_trace_raw_for_test`] can inject
/// an explicit override that bypasses the cache entirely.
static TRACE_RAW_CACHE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

fn trace_raw_enabled() -> bool {
    #[cfg(test)]
    {
        // Check the test override first; if set, bypass the production cache.
        if let Some(v) = *TEST_TRACE_RAW_OVERRIDE.lock().unwrap() {
            return v;
        }
    }

    *TRACE_RAW_CACHE.get_or_init(|| {
        // Any non-empty value enables raw mode; "1" is the documented value.
        matches!(
            std::env::var("FF_RDP_TRACE_RAW").as_deref(),
            Ok(s) if !s.is_empty()
        )
    })
}

/// Override the [`trace_raw_enabled`] result for the duration of a test.
///
/// Pass `Some(true)` or `Some(false)` to force a value, or `None` to clear
/// the override and fall back to the production cache / env var.  Callers
/// should hold [`ENV_LOCK`] for the duration of the test to prevent races.
#[cfg(test)]
pub(crate) fn set_trace_raw_for_test(value: Option<bool>) {
    *TEST_TRACE_RAW_OVERRIDE.lock().unwrap() = value;
}

#[cfg(test)]
static TEST_TRACE_RAW_OVERRIDE: std::sync::Mutex<Option<bool>> = std::sync::Mutex::new(None);

fn redact_inner(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut out = serde_json::Map::with_capacity(map.len());
            for (k, v) in map {
                let key_lower = k.to_lowercase();
                let is_sensitive = SENSITIVE_KEYS.iter().any(|s| *s == key_lower);
                let is_source = SOURCE_KEYS.iter().any(|s| *s == key_lower);
                let redacted_v = if is_sensitive || is_source {
                    redact_string_value(v)
                } else {
                    redact_inner(v)
                };
                out.insert(k.clone(), redacted_v);
            }
            Value::Object(out)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(redact_inner).collect()),
        Value::String(s) => {
            if s.len() > redact_threshold() {
                Value::String(format!("<redacted len={}>", s.len()))
            } else {
                value.clone()
            }
        }
        _ => value.clone(),
    }
}

fn redact_string_value(value: &Value) -> Value {
    match value {
        Value::String(s) => Value::String(format!("<redacted len={}>", s.len())),
        // Redact nested structures too — e.g. cookie arrays.
        Value::Array(arr) => Value::Array(arr.iter().map(redact_string_value).collect()),
        _ => Value::String(format!("<redacted len={}>", value.to_string().len())),
    }
}

// ---------------------------------------------------------------------------
// Tracing helpers
// ---------------------------------------------------------------------------

/// Extract the `"to"` or `"from"` actor field from a JSON packet for tracing.
fn packet_actor(packet: &Value) -> &str {
    packet
        .get("to")
        .or_else(|| packet.get("from"))
        .and_then(Value::as_str)
        .unwrap_or("-")
}

/// Extract the packet type field for tracing (`"type"` for requests, `"from"`
/// actor is in the response but the type may be missing — fall back to "-").
fn packet_kind(packet: &Value) -> &str {
    packet.get("type").and_then(Value::as_str).unwrap_or("-")
}

/// Default cap on frame payload size accepted from a Firefox RDP peer.
///
/// 256 MiB comfortably accommodates heap-snapshot dumps and other large
/// legitimate transfers (full-page screenshot data URLs are ≪ this).  Frames
/// declaring a larger length are rejected before any allocation is
/// attempted, preventing a malformed or malicious peer from causing an
/// immediate OOM abort.  Override at runtime with [`set_max_frame_bytes`].
///
/// Note: the receive parser checks the declared length against this cap
/// **before** allocating the body buffer, so an oversized declaration costs
/// only a few bytes of length-prefix parsing.
pub const DEFAULT_MAX_FRAME_BYTES: usize = 256 * 1024 * 1024;

/// Runtime-configurable frame-size cap.  `0` means "unset, use the
/// [`DEFAULT_MAX_FRAME_BYTES`]".
static MAX_FRAME_BYTES_CELL: AtomicUsize = AtomicUsize::new(0);

/// Set the maximum frame payload size in bytes accepted by [`recv_from`].
///
/// Intended to be called once at process startup (e.g. from the CLI front
/// end after parsing `--max-frame-mb`).  Calling at runtime is safe — the
/// new cap applies to the next frame read — but typically not needed.
///
/// `bytes = 0` resets to [`DEFAULT_MAX_FRAME_BYTES`].
pub fn set_max_frame_bytes(bytes: usize) {
    MAX_FRAME_BYTES_CELL.store(bytes, Ordering::Relaxed);
}

/// Current cap on frame payload size in bytes.
///
/// This is the *default* cap, read from the process-global cell. Every frame
/// parser in this module also has a `*_with_cap` form that takes the cap as an
/// argument; prefer those anywhere the cap needs to be something other than
/// the process-wide setting (notably in tests — see [`RaisedFrameCap`]).
pub fn max_frame_bytes() -> usize {
    resolve_frame_cap(MAX_FRAME_BYTES_CELL.load(Ordering::Relaxed))
}

/// Map a raw [`MAX_FRAME_BYTES_CELL`] value to the effective cap: `0` means
/// "unset, use [`DEFAULT_MAX_FRAME_BYTES`]".
///
/// Split out from [`max_frame_bytes`] so the `0` → default rule can be tested
/// without touching the process-global cell.
const fn resolve_frame_cap(raw: usize) -> usize {
    if raw == 0 {
        DEFAULT_MAX_FRAME_BYTES
    } else {
        raw
    }
}

/// Legacy alias for the default frame-size cap.  Prefer
/// [`max_frame_bytes`] in new code so the runtime knob is honoured.
#[deprecated(note = "use max_frame_bytes() to honour the --max-frame-mb runtime knob")]
pub const MAX_FRAME_BYTES: usize = DEFAULT_MAX_FRAME_BYTES;

/// Low-level transport for the Firefox Remote Debugging Protocol.
///
/// Firefox uses a simple length-prefixed JSON framing over TCP:
/// - **Send**: `{byte_length}:{json_payload}`
/// - **Recv**: read ASCII digits until `:`, interpret as the byte count, then
///   read exactly that many bytes and parse as JSON.
pub struct RdpTransport {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
    /// Optional sink for packets that arrive on the reply-channel but are in
    /// fact server-pushed events (e.g. `consoleAPICall`, `tabNavigated`).
    ///
    /// Set via [`set_event_sink`](Self::set_event_sink); when unset, stray
    /// events encountered by [`recv_reply_from`] are silently dropped (the
    /// pre-iter-69 behaviour). See `kb/rdp/protocol/message-format.md` —
    /// replies have no `type` field, every `from`+`type` packet is an event.
    event_sink: Option<Sender<Value>>,
    /// `innerWindowId` of the target the caller is currently talking to, when
    /// it has asked the wait loops to watch for that target being destroyed.
    ///
    /// See [`set_target_guard`](RdpTransport::set_target_guard).  `None` (the
    /// default) leaves the loops behaving exactly as they did before iter-220.
    target_guard: Option<u64>,
    /// Destination URL of the most recent top-level navigation Firefox
    /// announced on this connection, latched by [`recv`](RdpTransport::recv).
    ///
    /// See [`take_navigation_started`](RdpTransport::take_navigation_started).
    navigation_started: Option<String>,
}

impl std::fmt::Debug for RdpTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RdpTransport").finish_non_exhaustive()
    }
}

impl RdpTransport {
    /// Open a raw TCP connection without reading the Firefox greeting.
    ///
    /// Use this when you need to inspect the greeting packet (e.g. in
    /// [`RdpConnection`](crate::connection::RdpConnection)). If you don't need
    /// the greeting, prefer [`connect`](Self::connect) which discards it.
    pub fn connect_raw(host: &str, port: u16, timeout: Duration) -> Result<Self, ProtocolError> {
        use std::net::ToSocketAddrs;

        let addrs: Vec<_> = (host, port)
            .to_socket_addrs()
            .map_err(ProtocolError::ConnectionFailed)?
            .collect();

        if addrs.is_empty() {
            return Err(ProtocolError::ConnectionFailed(std::io::Error::new(
                std::io::ErrorKind::AddrNotAvailable,
                format!("could not resolve {host}:{port}"),
            )));
        }

        let mut last_err = None;
        for addr in &addrs {
            match TcpStream::connect_timeout(addr, timeout) {
                Ok(stream) => {
                    stream
                        .set_read_timeout(Some(timeout))
                        .map_err(ProtocolError::ConnectionFailed)?;
                    stream
                        .set_write_timeout(Some(timeout))
                        .map_err(ProtocolError::ConnectionFailed)?;
                    let reader = BufReader::new(
                        stream
                            .try_clone()
                            .map_err(ProtocolError::ConnectionFailed)?,
                    );
                    return Ok(Self {
                        reader,
                        writer: stream,
                        event_sink: None,
                        target_guard: None,
                        navigation_started: None,
                    });
                }
                Err(e) => {
                    last_err = Some(if e.kind() == std::io::ErrorKind::TimedOut {
                        ProtocolError::Timeout
                    } else {
                        ProtocolError::ConnectionFailed(e)
                    });
                }
            }
        }

        Err(last_err.unwrap_or_else(|| {
            ProtocolError::ConnectionFailed(std::io::Error::new(
                std::io::ErrorKind::AddrNotAvailable,
                format!("could not resolve {host}:{port}"),
            ))
        }))
    }

    /// Connect to a Firefox RDP server and consume the initial greeting packet.
    ///
    /// Firefox immediately sends a greeting after the TCP connection is
    /// established. We read and discard it so that the first call to
    /// [`recv`](Self::recv) returns an application-level message.
    ///
    /// The read timeout set on the socket handles the greeting timeout — no
    /// separate wrapper is needed.
    pub fn connect(host: &str, port: u16, timeout: Duration) -> Result<Self, ProtocolError> {
        let mut transport = Self::connect_raw(host, port, timeout)?;

        // Discard the Firefox greeting packet; socket read timeout applies.
        transport.recv()?;

        Ok(transport)
    }

    /// Build a transport from pre-existing reader/writer handles.
    ///
    /// Useful in tests where you already have a connected `TcpStream`.
    #[cfg(test)]
    pub(crate) fn from_parts(reader: BufReader<TcpStream>, writer: TcpStream) -> Self {
        Self {
            reader,
            writer,
            event_sink: None,
            target_guard: None,
            navigation_started: None,
        }
    }

    /// Decompose into the underlying reader/writer halves.
    ///
    /// Called by [`split`](Self::split) to hand the halves to `FramedReader`/`FramedWriter`.
    fn into_parts(self) -> (BufReader<TcpStream>, TcpStream) {
        (self.reader, self.writer)
    }

    /// Install (or clear) the side-channel for stray events encountered by
    /// [`recv_reply_from`].
    ///
    /// When a packet arrives with `from == actor` AND a `type` field (the
    /// protocol marker for an event), the helper forwards it to this sender
    /// instead of mis-classifying it as the reply. Pass `None` to disable.
    ///
    /// If the receiver has been dropped the event is silently discarded —
    /// the reply loop must never block on a slow consumer.
    pub fn set_event_sink(&mut self, sink: Option<Sender<Value>>) {
        self.event_sink = sink;
    }

    /// Install a new event sink and return the previous one.
    ///
    /// Unlike [`set_event_sink`](Self::set_event_sink) this preserves whatever
    /// sink was already installed so a caller can temporarily capture push
    /// events (e.g. a `resources-available-array` that `recv_reply_from` routes
    /// to the sink while awaiting a `watchResources` ACK) and then restore the
    /// original sink — without silently clobbering a daemon-installed one.
    pub fn swap_event_sink(&mut self, sink: Option<Sender<Value>>) -> Option<Sender<Value>> {
        std::mem::replace(&mut self.event_sink, sink)
    }

    /// Watch for Firefox destroying the target bound to `inner_window_id`
    /// while a reply or event wait is in flight (iter-220).
    ///
    /// A `click` on a link starts a navigation that tears the current docshell
    /// down.  Any request already sent to that docshell's actors is silently
    /// dropped — Firefox neither answers it nor reports an error — so
    /// [`recv_reply_from`] / [`recv_event_from`] block until the socket read
    /// timeout, and the caller reports `operation timed out (phase: recv)`
    /// having burned its whole `--timeout` budget.
    ///
    /// Firefox *does* announce the teardown, on the watcher actor, as
    /// `{"type": "target-destroyed-form", "target": {"innerWindowId": N}}`.
    /// With the guard set to `N` the wait loops turn that announcement into
    /// [`ProtocolError::EvalTargetDestroyed`] the moment it arrives, which in
    /// practice is tens of milliseconds — early enough for the caller to
    /// re-resolve the target and retry against the new document.
    ///
    /// The guard is **opt-in and narrow on purpose**: aborting a wait is only
    /// correct where the caller is prepared to retry.  A caller that sets it
    /// must clear it (`None`) as soon as its guarded section ends, or a later
    /// unrelated wait on the same connection inherits the abort.
    pub fn set_target_guard(&mut self, inner_window_id: Option<u64>) {
        self.target_guard = inner_window_id;
    }

    /// The `innerWindowId` currently guarded, if any.
    ///
    /// Lets a caller save and restore the guard around a nested section
    /// instead of clobbering an outer one.
    pub fn target_guard(&self) -> Option<u64> {
        self.target_guard
    }

    /// Take (and clear) the destination URL of the most recent top-level
    /// navigation Firefox announced on this connection (iter-220).
    ///
    /// Firefox pushes `{"type": "tabNavigated", "state": "start", "url": …}`
    /// — or `willNavigate` — the moment a link click, form submit, or
    /// `location` assignment starts loading a new top-level document.  That
    /// announcement is the only *positive* evidence a command has that the
    /// action it just performed navigated: `getTarget` keeps handing back the
    /// outgoing docshell for a while afterwards, so comparing target actor IDs
    /// cannot tell the difference between "nothing happened" and "the document
    /// is about to be replaced".
    ///
    /// [`recv`](Self::recv) latches it because that is the single choke point
    /// every packet passes through, including the ones the reply/event loops
    /// forward to the event sink or drop.  Returns `None` when no navigation
    /// has been announced since the last call.
    pub fn take_navigation_started(&mut self) -> Option<String> {
        self.navigation_started.take()
    }

    /// Internal accessor used by [`recv_reply_from`] / [`recv_event_from`].
    fn forward_event(&self, event: Value) {
        if let Some(tx) = &self.event_sink {
            // Ignore SendError: a dropped receiver just means the subscriber
            // went away; the reply loop must continue regardless.
            let _ = tx.send(event);
        }
    }

    /// Split the transport into typed framed halves.
    ///
    /// The returned [`FramedReader`] and [`FramedWriter`] share the same underlying
    /// TCP connection. The read half is exclusive; the write half can be shared
    /// via the calling thread. Both halves speak the Firefox RDP framing protocol.
    ///
    /// This is the preferred way for the daemon to split the connection so it
    /// never needs to import raw `encode_frame`/`recv_from` from this crate.
    pub fn split(self) -> (FramedReader, FramedWriter) {
        let (reader, writer) = self.into_parts();
        (FramedReader { reader }, FramedWriter { writer })
    }

    /// Override the socket read timeout.
    ///
    /// Pass `None` to block indefinitely (not recommended in production).
    /// This is used by commands that need a different idle-detection window
    /// than the one established at connect time (e.g. `navigate --with-network`
    /// with a shorter `--network-timeout`).
    ///
    /// Sets the timeout on both the reader and writer halves.  On most
    /// platforms `SO_RCVTIMEO` is a socket-level option shared across cloned
    /// handles, but setting it on both is the safe, cross-platform approach.
    pub fn set_read_timeout(&mut self, timeout: Option<Duration>) -> Result<(), ProtocolError> {
        self.reader
            .get_mut()
            .set_read_timeout(timeout)
            .map_err(ProtocolError::ConnectionFailed)?;
        self.writer
            .set_read_timeout(timeout)
            .map_err(ProtocolError::ConnectionFailed)
    }

    /// Read the socket's current read timeout (`SO_RCVTIMEO`), as set by the
    /// most recent [`set_read_timeout`](Self::set_read_timeout) call or by
    /// the initial connect.
    ///
    /// iter-129: added so a caller that temporarily narrows the read timeout
    /// (e.g. [`crate::actors::watcher::enumerate_frame_targets`]'s short poll
    /// interval while draining target events) can restore the **exact**
    /// prior value afterwards instead of guessing or clearing it to `None`.
    /// Clearing to `None` when the caller's connection was actually
    /// configured with a finite timeout silently makes every subsequent
    /// `recv()` block forever instead of erroring — confirmed live: an
    /// `evaluateJSAsync` call issued after `enumerate_frame_targets` returned
    /// hung indefinitely (no `ProtocolError::Timeout`) until this was fixed.
    pub fn read_timeout(&self) -> Result<Option<Duration>, ProtocolError> {
        self.reader
            .get_ref()
            .read_timeout()
            .map_err(ProtocolError::ConnectionFailed)
    }

    /// Send a JSON message using Firefox RDP framing: `{len}:{json}`.
    pub fn send(&mut self, message: &Value) -> Result<(), ProtocolError> {
        let json = serde_json::to_string(message)
            .map_err(|e| ProtocolError::InvalidPacket(e.to_string()))?;

        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!(
                target: "ff_rdp_core::transport",
                direction = "send",
                actor = %packet_actor(message),
                kind = %packet_kind(message),
                payload_size = json.len(),
                body = %serde_json::to_string(&redact(message)).unwrap_or_default(),
            );
        }

        let frame = encode_frame(&json);
        self.writer
            .write_all(frame.as_bytes())
            .map_err(map_send_io_error)?;

        Ok(())
    }

    /// Receive a single length-prefixed JSON message.
    pub fn recv(&mut self) -> Result<Value, ProtocolError> {
        let value = recv_from(&mut self.reader)?;

        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!(
                target: "ff_rdp_core::transport",
                direction = "recv",
                actor = %packet_actor(&value),
                kind = %packet_kind(&value),
                payload_size = serde_json::to_string(&value).map_or(0, |s| s.len()),
                body = %serde_json::to_string(&redact(&value)).unwrap_or_default(),
            );
        }

        // iter-220: latch top-level navigation announcements here — the one
        // choke point every packet passes through, including the events the
        // reply/event loops forward to the sink or drop on the floor.
        if let Some(url) = navigation_start_url(&value) {
            self.navigation_started = Some(url);
        }

        Ok(value)
    }

    // NOTE (iter-102): the blind `pub fn request(&mut self, &Value)` —
    // send + one *unmatched* `recv` — was removed here.  It bypassed
    // `recv_reply_from`, so a push event (e.g. a `tabNavigated` fired during a
    // reload) arriving before the reply would be consumed as the reply,
    // desyncing the actor's reply stream (the bug class iter-69/74 eliminated
    // everywhere else).  Its last caller, `TargetFront::reload(force=true)`,
    // now goes through `actor::actor_request`, which matches the reply via
    // `recv_reply_from` and routes interleaved pushes to the event sink.  New
    // code must use `actor_request` / `actor_send` (matched) or the typed
    // `specs::call` path — never a raw send-then-recv.

    /// Send a fire-and-forget (oneway) typed packet to an actor.
    ///
    /// Builds `{"to": to, "type": type_, ...body}`, sends it, and returns
    /// **without** reading any reply.  Use this for Firefox RDP methods declared
    /// `oneway: true` in the spec (e.g. `unwatchTargets`, `clearResources`,
    /// `reflow.start`).  Awaiting a reply for these methods would hang until the
    /// socket read timeout because Firefox never sends one.
    ///
    /// `body` may be `Value::Null` or `Value::Object({})` for methods that take
    /// no extra parameters.
    pub fn actor_send_oneway(
        &mut self,
        to: &str,
        type_: &str,
        body: Value,
    ) -> Result<(), ProtocolError> {
        // Build the packet map directly so the `to`/`type` fields are inserted
        // without a re-assert of the object shape (avoids a production
        // `.expect()`).
        let mut obj = match body {
            Value::Object(map) => map,
            Value::Null => serde_json::Map::new(),
            other => {
                return Err(ProtocolError::InvalidPacket(format!(
                    "actor_send_oneway: body must be an object or null, got: {other}"
                )));
            }
        };
        obj.insert("to".into(), Value::String(to.to_owned()));
        obj.insert("type".into(), Value::String(type_.to_owned()));
        self.send(&Value::Object(obj))
    }

    /// Receive a bulk packet from `actor` with kind `kind`, streaming bytes
    /// directly into `out` in 8 KiB chunks without buffering the full body.
    ///
    /// Firefox's bulk-frame wire format is:
    /// `bulk <actor> <kind> <length>:<binary-data>`
    ///
    /// This method reads the next frame from the transport.  If it is a bulk
    /// frame whose `actor` and `kind` fields match the expected values, the
    /// body bytes are copied to `out` in [`BULK_CHUNK_SIZE`] chunks and the
    /// total byte count is returned.  If the frame is a JSON packet or a bulk
    /// frame from a different actor/kind, `Err(ProtocolError::BulkPacketUnexpected)`
    /// is returned.
    ///
    /// The bulk body is limited by `max_frame_bytes()`.  An announcement
    /// exceeding the cap returns `ProtocolError::BulkFrameTooLarge` before any
    /// allocation is attempted.
    pub fn recv_bulk_with_handler<W: Write>(
        &mut self,
        actor: &str,
        kind: &str,
        out: &mut W,
    ) -> Result<u64, ProtocolError> {
        recv_bulk_with_handler_from(&mut self.reader, actor, kind, out)
    }
}

// ---------------------------------------------------------------------------
// Typed split halves
// ---------------------------------------------------------------------------

/// Read half of a split [`RdpTransport`].
///
/// Owned exclusively by the Firefox-reader thread in the daemon.
pub struct FramedReader {
    reader: BufReader<TcpStream>,
}

impl FramedReader {
    /// Wrap a `TcpStream` in a `FramedReader` without going through [`RdpTransport`].
    ///
    /// Useful in the daemon where client TCP streams need to be read using the
    /// typed framing API rather than the raw `recv_from` free function.
    pub fn from_stream(stream: TcpStream) -> Self {
        Self {
            reader: BufReader::new(stream),
        }
    }

    /// Receive a single length-prefixed JSON frame.
    ///
    /// Mirrors [`RdpTransport::recv`].
    pub fn recv(&mut self) -> Result<Value, ProtocolError> {
        let value = recv_from(&mut self.reader)?;

        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!(
                target: "ff_rdp_core::transport",
                direction = "recv",
                actor = %packet_actor(&value),
                kind = %packet_kind(&value),
                payload_size = serde_json::to_string(&value).map_or(0, |s| s.len()),
                body = %serde_json::to_string(&redact(&value)).unwrap_or_default(),
            );
        }

        Ok(value)
    }

    /// Set the read timeout on the underlying socket.
    pub fn set_read_timeout(&self, timeout: Option<Duration>) -> Result<(), ProtocolError> {
        self.reader
            .get_ref()
            .set_read_timeout(timeout)
            .map_err(ProtocolError::ConnectionFailed)
    }

    /// Try to clone the underlying `TcpStream`.
    ///
    /// The clone shares the same underlying socket. Useful when the daemon
    /// needs to hand a write clone to a `StreamSubscriber` while retaining the
    /// read half for the client loop.
    pub fn try_clone_stream(&self) -> std::io::Result<TcpStream> {
        self.reader.get_ref().try_clone()
    }

    /// Receive a bulk packet streaming directly into `out`.
    ///
    /// Mirrors [`RdpTransport::recv_bulk_with_handler`]; see its documentation
    /// for the full contract.
    pub fn recv_bulk_with_handler<W: Write>(
        &mut self,
        actor: &str,
        kind: &str,
        out: &mut W,
    ) -> Result<u64, ProtocolError> {
        recv_bulk_with_handler_from(&mut self.reader, actor, kind, out)
    }
}

/// Write half of a split [`RdpTransport`].
///
/// Can be wrapped in `Arc<Mutex<_>>` for shared write access across threads.
pub struct FramedWriter {
    writer: TcpStream,
}

impl FramedWriter {
    /// Wrap a `TcpStream` in a `FramedWriter` without going through [`RdpTransport`].
    ///
    /// Useful in the daemon where client TCP streams need to be written using the
    /// typed framing API rather than the raw `encode_frame` free function.
    pub fn from_stream(stream: TcpStream) -> Self {
        Self { writer: stream }
    }

    /// Send a JSON value using Firefox RDP framing: `{len}:{json}`.
    ///
    /// Mirrors [`RdpTransport::send`].
    pub fn send(&mut self, message: &Value) -> Result<(), ProtocolError> {
        let json = serde_json::to_string(message)
            .map_err(|e| ProtocolError::InvalidPacket(e.to_string()))?;

        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!(
                target: "ff_rdp_core::transport",
                direction = "send",
                actor = %packet_actor(message),
                kind = %packet_kind(message),
                payload_size = json.len(),
                body = %serde_json::to_string(&redact(message)).unwrap_or_default(),
            );
        }

        let frame = encode_frame(&json);
        self.writer
            .write_all(frame.as_bytes())
            .map_err(map_send_io_error)
    }

    /// Send a pre-serialised JSON string as a Firefox RDP frame.
    ///
    /// Use this when you already have the JSON string and want to avoid a
    /// redundant parse/serialise round-trip.
    pub fn send_raw(&mut self, json: &str) -> Result<(), ProtocolError> {
        let frame = encode_frame(json);
        self.writer
            .write_all(frame.as_bytes())
            .map_err(map_send_io_error)
    }

    /// Try to clone the underlying `TcpStream`.
    ///
    /// The clone shares the same underlying socket; writes to either handle
    /// go to the same peer.  Useful when a write half must be handed to a
    /// subscriber without consuming the original.
    pub fn try_clone_stream(&self) -> std::io::Result<TcpStream> {
        self.writer.try_clone()
    }
}

// ---------------------------------------------------------------------------
// Bulk streaming
// ---------------------------------------------------------------------------

/// Chunk size used when streaming bulk packet bodies to an output writer.
const BULK_CHUNK_SIZE: usize = 8 * 1024; // 8 KiB

/// Discard exactly `length` bytes from `reader` in 8 KiB chunks.
///
/// Used by [`recv_bulk_with_handler_from`] and [`drain_bulk_frame_with_cap`] to consume
/// a mismatched or unsupported bulk frame body so the stream stays aligned.
fn drain_bulk_body<R: Read>(reader: &mut R, length: u64) -> Result<(), ProtocolError> {
    let mut remaining = length;
    let mut chunk = vec![0u8; BULK_CHUNK_SIZE];
    while remaining > 0 {
        let to_read = usize::try_from(remaining)
            .unwrap_or(BULK_CHUNK_SIZE)
            .min(BULK_CHUNK_SIZE);
        reader
            .read_exact(&mut chunk[..to_read])
            .map_err(map_recv_io_error)?;
        remaining -= to_read as u64;
    }
    Ok(())
}

/// Consume a complete bulk frame from `reader` whose first byte (`b`) has
/// already been consumed by the caller.
///
/// Reads the rest of the header (`ulk <actor> <kind> <length>:`), validates it,
/// applies the `cap`, then reads-and-discards exactly `length` bytes from the
/// body.  Returns `Ok((actor, kind, length))` when the frame has been fully
/// consumed so the caller can continue reading the next frame.
///
/// This is the low-level drain behind [`recv_bulk_frame`] (which returns
/// [`ProtocolError::BulkPacketUnsupported`]); a reader loop that meets an
/// unexpected bulk frame mid-stream must drain it the same way to keep the TCP
/// stream aligned.
///
/// The cap is a parameter rather than a read of the process-global cell so a
/// caller — in practice a test — can exercise it without mutating shared state
/// that every other frame parse in the process reads; see [`RaisedFrameCap`].
/// Callers wanting the configured cap pass [`max_frame_bytes()`].
///
/// Errors:
/// - `InvalidPacket` — malformed header.
/// - `BulkFrameTooLarge` — announced length exceeds `cap` (body is NOT read in
///   this case, so the stream is unrecoverable).
/// - `RecvFailed` — I/O error while reading.
pub(crate) fn drain_bulk_frame_with_cap<R: BufRead>(
    reader: &mut R,
    first_byte: u8,
    cap: usize,
) -> Result<(String, String, u64), ProtocolError> {
    // Re-assemble the header starting from the already-consumed first byte.
    let mut header_buf: Vec<u8> = vec![first_byte];
    loop {
        let mut byte = [0u8; 1];
        reader.read_exact(&mut byte).map_err(map_recv_io_error)?;
        if byte[0] == b':' {
            break;
        }
        header_buf.push(byte[0]);
        if header_buf.len() > 4096 {
            return Err(ProtocolError::InvalidPacket(
                "bulk frame header exceeds 4096 bytes".to_owned(),
            ));
        }
    }

    let header = std::str::from_utf8(&header_buf)
        .map_err(|_| ProtocolError::InvalidPacket("non-UTF8 in bulk frame header".to_owned()))?;

    // Expected: "bulk <actor> <kind> <length>"
    let parts: Vec<&str> = header.splitn(4, ' ').collect();
    if parts.len() != 4 || parts[0] != "bulk" {
        return Err(ProtocolError::InvalidPacket(format!(
            "malformed bulk frame header: {header:?}"
        )));
    }
    let actor = parts[1].to_owned();
    let kind = parts[2].to_owned();
    let length: u64 = parts[3]
        .parse()
        .map_err(|e| ProtocolError::InvalidPacket(format!("bulk length parse error: {e}")))?;

    // Cap check before entering the discard loop.
    let cap = cap as u64;
    if length > cap {
        return Err(ProtocolError::BulkFrameTooLarge {
            announced: length,
            max: cap,
        });
    }

    drain_bulk_body(reader, length)?;
    Ok((actor, kind, length))
}

/// Receive a bulk frame from `reader`, matching the expected `actor` and `kind`.
///
/// Uses `BufRead::fill_buf` to peek the first byte without consuming it.  If
/// the first byte is not `b` (indicating a JSON frame rather than a bulk frame),
/// the byte is **not** consumed and the function returns
/// `Err(ProtocolError::BulkPacketUnexpected)`.  The stream stays aligned so
/// the caller's next `recv_from` reads the JSON frame intact.
///
/// On actor/kind mismatch (after parsing the header), the body is discarded via
/// [`drain_bulk_body`] before returning `BulkPacketUnexpected`, keeping the
/// stream aligned.
///
/// Errors:
/// - `BulkFrameTooLarge` — announced length exceeds `max_frame_bytes()`.
/// - `BulkPacketUnexpected` — actor/kind mismatch, or the next frame is a JSON
///   packet rather than a bulk packet.
/// - `InvalidPacket` — malformed header.
/// - `RecvFailed` / `Timeout` — I/O error while reading.
fn recv_bulk_with_handler_from<W: Write, R: BufRead>(
    reader: &mut R,
    actor: &str,
    kind: &str,
    out: &mut W,
) -> Result<u64, ProtocolError> {
    recv_bulk_with_handler_from_with_cap(reader, actor, kind, out, max_frame_bytes())
}

/// [`recv_bulk_with_handler_from`] with the frame-size cap supplied by the
/// caller instead of read from the process-global cell.  See
/// [`drain_bulk_frame_with_cap`] for why this form exists.
fn recv_bulk_with_handler_from_with_cap<W: Write, R: BufRead>(
    reader: &mut R,
    actor: &str,
    kind: &str,
    out: &mut W,
    cap: usize,
) -> Result<u64, ProtocolError> {
    // Peek the first byte WITHOUT consuming it.
    {
        let buf = reader.fill_buf().map_err(map_recv_io_error)?;
        if buf.is_empty() {
            return Err(ProtocolError::RecvFailed(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "EOF before bulk frame",
            )));
        }
        if buf[0] != b'b' {
            // JSON frame peeked — do NOT consume; the stream stays aligned.
            return Err(ProtocolError::BulkPacketUnexpected {
                actor: actor.to_owned(),
                kind: kind.to_owned(),
            });
        }
    }
    // Consume the `b` byte we peeked.
    reader.consume(1);

    // Read the rest of the header up to ':'.  We already consumed 'b'.
    let mut header_buf: Vec<u8> = b"b".to_vec();
    loop {
        let mut byte = [0u8; 1];
        reader.read_exact(&mut byte).map_err(map_recv_io_error)?;
        if byte[0] == b':' {
            break;
        }
        header_buf.push(byte[0]);
        if header_buf.len() > 4096 {
            return Err(ProtocolError::InvalidPacket(
                "bulk frame header exceeds 4096 bytes".to_owned(),
            ));
        }
    }

    let header = std::str::from_utf8(&header_buf)
        .map_err(|_| ProtocolError::InvalidPacket("non-UTF8 in bulk frame header".to_owned()))?;

    // Expected: "bulk <actor> <kind> <length>"
    let parts: Vec<&str> = header.splitn(4, ' ').collect();
    if parts.len() != 4 || parts[0] != "bulk" {
        return Err(ProtocolError::InvalidPacket(format!(
            "malformed bulk frame header: {header:?}"
        )));
    }
    let frame_actor = parts[1];
    let frame_kind = parts[2];
    let length: u64 = parts[3]
        .parse()
        .map_err(|e| ProtocolError::InvalidPacket(format!("bulk length parse error: {e}")))?;

    // Validate cap before any I/O.
    let cap = cap as u64;
    if length > cap {
        return Err(ProtocolError::BulkFrameTooLarge {
            announced: length,
            max: cap,
        });
    }

    // Validate actor/kind match.  On mismatch, drain the body first so the
    // stream stays aligned, then return the typed error.
    if frame_actor != actor || frame_kind != kind {
        drain_bulk_body(reader, length)?;
        return Err(ProtocolError::BulkPacketUnexpected {
            actor: actor.to_owned(),
            kind: kind.to_owned(),
        });
    }

    // Stream body into `out` in chunks.
    let mut remaining = length;
    let mut chunk = vec![0u8; BULK_CHUNK_SIZE];
    while remaining > 0 {
        // Safe: remaining <= BULK_CHUNK_SIZE (usize) after the .min() so the
        // truncation on 32-bit targets cannot actually occur.  We use
        // try_from + unwrap_or to silence the cast lint cleanly.
        let to_read = usize::try_from(remaining)
            .unwrap_or(BULK_CHUNK_SIZE)
            .min(BULK_CHUNK_SIZE);
        reader
            .read_exact(&mut chunk[..to_read])
            .map_err(map_recv_io_error)?;
        out.write_all(&chunk[..to_read])
            .map_err(ProtocolError::SendFailed)?;
        remaining -= to_read as u64;
    }

    Ok(length)
}

// ---------------------------------------------------------------------------
// Reply / event matching helpers (iter-69)
// ---------------------------------------------------------------------------

/// Read packets from `transport` until the **reply** from `actor` arrives.
///
/// A reply is identified per the Firefox RDP rule
/// (`kb/rdp/protocol/message-format.md`): `from == actor` AND **no** `type`
/// field. Any packet with `from == actor && type == Some(_)` is an event
/// (e.g. `consoleAPICall`, `tabNavigated`); these are forwarded to the
/// transport's event sink (see [`RdpTransport::set_event_sink`]) and the
/// loop continues.
///
/// Packets from **other** actors are also forwarded to the event sink (iter-74
/// fix: sibling-actor packets must not be silently dropped — they may be
/// watcher events, console events, or other push notifications that arrived
/// while a request was in-flight).
///
/// On `error`-bearing replies, the helper converts the packet into a
/// [`ProtocolError::ActorError`] using [`ActorErrorKind::from_code`].
/// Extract the destination URL from a top-level navigation-start announcement.
///
/// Firefox pushes two shapes for "a new top-level document is on its way":
/// `{"type": "willNavigate", "url": …}` and
/// `{"type": "tabNavigated", "state": "start", "url": …}`.  A `tabNavigated`
/// with `state: "stop"` reports a navigation that has *finished* and is not a
/// start announcement.  Sub-frame loads travel as `frameUpdate`, not
/// `tabNavigated`, so this never fires for an iframe.
///
/// Returns the URL string when the packet is a start announcement, `None`
/// otherwise.  A start announcement with no `url` still counts, reported as an
/// empty string, because the fact that a navigation began is the signal —
/// the URL only refines it.
fn navigation_start_url(msg: &Value) -> Option<String> {
    let ty = msg.get("type").and_then(Value::as_str)?;
    let is_start = match ty {
        "willNavigate" => true,
        "tabNavigated" => msg.get("state").and_then(Value::as_str) != Some("stop"),
        _ => false,
    };
    if !is_start {
        return None;
    }
    Some(
        msg.get("url")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
    )
}

/// Recognise Firefox's announcement that the guarded target has been destroyed.
///
/// Returns `Some(ProtocolError::EvalTargetDestroyed)` only when a guard is
/// installed (see [`RdpTransport::set_target_guard`]) *and* the incoming packet
/// is a `target-destroyed-form` naming exactly that `innerWindowId`.  Every
/// other packet — including `target-destroyed-form` for some *other* document,
/// which is routine on a page with iframes — returns `None` and is handled
/// normally.
fn target_destroyed_guard_hit(msg: &Value, guard: Option<u64>) -> Option<ProtocolError> {
    let guard = guard?;
    // A top-level navigation started. Whatever the guarded target is still
    // computing describes a document that is on its way out, and once its
    // docshell goes Firefox drops the pending request without a word — so stop
    // waiting now while the caller still has budget to re-resolve and retry.
    // Only a guarded section asks for this, and a guarded section is by
    // definition one the caller is prepared to redo.
    if navigation_start_url(msg).is_some() {
        return Some(ProtocolError::EvalTargetDestroyed {
            inner_window_id: guard,
        });
    }
    if msg.get("type").and_then(Value::as_str)? != "target-destroyed-form" {
        return None;
    }
    let destroyed = msg
        .get("target")?
        .get("innerWindowId")
        .and_then(Value::as_u64)?;
    (destroyed == guard).then_some(ProtocolError::EvalTargetDestroyed {
        inner_window_id: guard,
    })
}

pub fn recv_reply_from(transport: &mut RdpTransport, actor: &str) -> Result<Value, ProtocolError> {
    loop {
        let msg = transport.recv()?;
        // iter-220: the actor we are waiting on may have just been destroyed by
        // a navigation; Firefox will never answer, so abort now rather than at
        // the socket read timeout.
        if let Some(err) = target_destroyed_guard_hit(&msg, transport.target_guard()) {
            return Err(err);
        }
        let from = msg.get("from").and_then(Value::as_str).unwrap_or_default();
        if from != actor {
            // iter-101 Theme B: a control-error frame injected by the daemon
            // (e.g. `daemon_busy` when a second client tried to use the RPC
            // channel) will never be followed by the awaited actor reply
            // because the request was *not* forwarded.  Surface it promptly as
            // an ActorError so the caller fails fast instead of blocking until
            // the socket timeout.
            if let Some(err) = daemon_control_error(&msg) {
                return Err(err);
            }
            // Sibling-actor packet — forward to the event sink so it isn't
            // lost (e.g. watcher events that arrived while we awaited a reply
            // on a different actor).
            transport.forward_event(msg);
            continue;
        }
        if msg.get("type").is_some() {
            // Right actor, but typed → this is a push event, not the reply.
            // Forward to the side channel and keep waiting.
            transport.forward_event(msg);
            continue;
        }
        if let Some(error) = msg.get("error").and_then(Value::as_str) {
            return Err(ProtocolError::ActorError {
                actor: from.to_owned(),
                kind: ActorErrorKind::from_code(error),
                error: error.to_owned(),
                message: msg
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
            });
        }
        return Ok(msg);
    }
}

/// Read packets from `transport` until a packet `m` satisfies
/// `from == actor && predicate(&m)`.
///
/// Designed for the `evaluationResult` / `tabNavigated` / `document-event`
/// wait loops where the caller picks a specific event among the push stream
/// from a known actor.
///
/// Packets from **other** actors are forwarded to the event sink (iter-74 fix:
/// they must not be silently dropped — the same applies to events from the
/// target actor that do not match the predicate, such as intermediate
/// `consoleAPICall` packets that arrive between an `evaluateJSAsync`
/// acknowledgement and the final `evaluationResult`).
///
/// If the target actor emits an `error`-bearing reply (no `type` field, per
/// the protocol) it is surfaced as [`ProtocolError::ActorError`] rather than
/// silently skipped — otherwise callers like [`ThreadActor::attach`] would
/// block until the socket timeout instead of seeing the real failure.
pub fn recv_event_from(
    transport: &mut RdpTransport,
    actor: &str,
    mut predicate: impl FnMut(&Value) -> bool,
) -> Result<Value, ProtocolError> {
    loop {
        let msg = transport.recv()?;
        // iter-220: see `recv_reply_from` — a destroyed target never replies.
        if let Some(err) = target_destroyed_guard_hit(&msg, transport.target_guard()) {
            return Err(err);
        }
        let from = msg.get("from").and_then(Value::as_str).unwrap_or_default();
        if from == actor {
            // A typed-less packet carrying `error` is an error reply from the
            // actor — terminal, never a transient event to skip.
            if msg.get("type").is_none()
                && let Some(error) = msg.get("error").and_then(Value::as_str)
            {
                return Err(ProtocolError::ActorError {
                    actor: from.to_owned(),
                    kind: ActorErrorKind::from_code(error),
                    error: error.to_owned(),
                    message: msg
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned(),
                });
            }
            if predicate(&msg) {
                return Ok(msg);
            }
            // Non-matching event from the target actor (e.g. an intermediate
            // `consoleAPICall` while waiting for `evaluationResult`) — forward
            // to the sink instead of discarding.
            transport.forward_event(msg);
        } else {
            // iter-101 Theme B: fail fast on a daemon control-error frame
            // (see `recv_reply_from`) rather than waiting for an actor reply
            // that will never arrive.
            if let Some(err) = daemon_control_error(&msg) {
                return Err(err);
            }
            // Packet from a sibling actor — forward to the sink.
            transport.forward_event(msg);
        }
    }
}

/// Recognise a daemon-injected control-error frame (iter-101 Theme B).
///
/// The ff-rdp daemon proxies raw Firefox RDP but occasionally needs to signal a
/// condition of its own (currently only `daemon_busy`) by emitting a frame with
/// `from == "daemon"` and an `error` field.  Because such a frame is *not* an
/// actor reply and will never be followed by one for the awaited request, the
/// reply/event wait loops convert it into a terminal [`ProtocolError::ActorError`]
/// (with `actor = "daemon"`) so the caller fails fast.
///
/// Returns `None` for any frame that is not a daemon control-error, so ordinary
/// forwarded `from == "daemon"` frames (e.g. the greeting) are unaffected.
fn daemon_control_error(msg: &Value) -> Option<ProtocolError> {
    if msg.get("from").and_then(Value::as_str) != Some("daemon") {
        return None;
    }
    let error = msg.get("error").and_then(Value::as_str)?;
    Some(ProtocolError::ActorError {
        actor: "daemon".to_owned(),
        kind: ActorErrorKind::from_code(error),
        error: error.to_owned(),
        message: msg
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned(),
    })
}

/// Encode a JSON string as a Firefox RDP frame: `"{len}:{json}"`.
pub fn encode_frame(json: &str) -> String {
    format!("{}:{}", json.len(), json)
}

/// Read a single length-prefixed JSON packet from `reader`.
///
/// Firefox RDP uses two frame formats:
///
/// 1. **JSON frames**: `<length>:<json>` — normal packets handled here.
/// 2. **Bulk frames**: `bulk <actor> <kind> <length>:<binary-data>` — binary
///    packets that begin with the ASCII letter `b`.  This implementation cannot
///    process their binary payload, so the body bytes are consumed (skipped) and
///    [`ProtocolError::BulkPacketUnsupported`] is returned.  The stream remains
///    valid; the caller can log the error once and continue reading.
pub fn recv_from(reader: &mut impl BufRead) -> Result<Value, ProtocolError> {
    recv_from_with_cap(reader, max_frame_bytes())
}

/// [`recv_from`] with the frame-size cap supplied by the caller instead of read
/// from the process-global cell.
///
/// The process-global cap is set once at startup from `--max-frame-mb`; this
/// form exists for callers — in practice tests — that need a different cap for
/// one parse without mutating state every other parse in the process reads.
/// See [`RaisedFrameCap`].
pub(crate) fn recv_from_with_cap(
    reader: &mut impl BufRead,
    cap: usize,
) -> Result<Value, ProtocolError> {
    // Read the first byte to distinguish JSON vs bulk frames.
    let mut first = [0u8; 1];
    reader.read_exact(&mut first).map_err(map_recv_io_error)?;

    if first[0] == b'b' {
        // Delegate to drain_bulk_frame_with_cap which shares the discard logic with
        // recv_bulk_with_handler_from.  recv_bulk_frame returns
        // BulkPacketUnsupported after draining; we map that back from the
        // existing helper.
        return recv_bulk_frame(reader, first[0], cap);
    }

    // Normal JSON frame: read remaining bytes of the length prefix.
    let mut length_buf = Vec::with_capacity(10);

    if first[0] == b':' {
        // Degenerate: length was empty.
        return Err(ProtocolError::InvalidPacket(
            "empty length prefix".to_owned(),
        ));
    }

    if !first[0].is_ascii_digit() {
        return Err(ProtocolError::InvalidPacket(format!(
            "unexpected byte {:#x} in length prefix",
            first[0]
        )));
    }
    length_buf.push(first[0]);

    loop {
        let mut byte = [0u8; 1];
        reader.read_exact(&mut byte).map_err(map_recv_io_error)?;

        if byte[0] == b':' {
            break;
        }

        if byte[0].is_ascii_digit() {
            length_buf.push(byte[0]);
        } else {
            return Err(ProtocolError::InvalidPacket(format!(
                "unexpected byte {:#x} in length prefix",
                byte[0]
            )));
        }

        // Guard against malformed streams with no ':' separator.
        if length_buf.len() >= 20 {
            return Err(ProtocolError::InvalidPacket(
                "length prefix is 20+ digits".to_owned(),
            ));
        }
    }

    let length_str = std::str::from_utf8(&length_buf)
        .map_err(|_| ProtocolError::InvalidPacket("non-UTF8 in length prefix".to_owned()))?;

    let length: usize = length_str
        .parse()
        .map_err(|e| ProtocolError::InvalidPacket(format!("length parse error: {e}")))?;

    // Reject oversized frames before allocating.  A peer that announces more
    // than the configured cap is either corrupted or malicious.
    if length > cap {
        return Err(ProtocolError::FrameTooLarge {
            declared: length,
            max: cap,
        });
    }

    let mut body = vec![0u8; length];
    reader.read_exact(&mut body).map_err(map_recv_io_error)?;

    let value = serde_json::from_slice(&body)
        .map_err(|e| ProtocolError::InvalidPacket(format!("JSON parse error: {e}")))?;

    Ok(value)
}

/// Validate that an outbound bulk-frame length is within the configured cap.
///
/// Even though this implementation does not currently emit bulk frames, the
/// guard exists so that if a sender path is added later (or a downstream
/// consumer wraps `FramedWriter`) it cannot accidentally enqueue a frame that
/// the receive side would refuse.  Matching the cap on both directions makes
/// "the largest frame ff-rdp will ship" the same number on the wire and in
/// memory profiles.
///
/// Returns [`ProtocolError::BulkFrameTooLarge`] when `length` exceeds `cap`;
/// otherwise `Ok(())`.  The cap is a parameter rather than a read of the
/// process-global cell so the caller can exercise it without mutating shared
/// state — see [`RaisedFrameCap`].
#[cfg(test)]
pub(crate) fn check_outbound_bulk_size(length: u64, cap: usize) -> Result<(), ProtocolError> {
    let cap = cap as u64;
    if length > cap {
        Err(ProtocolError::BulkFrameTooLarge {
            announced: length,
            max: cap,
        })
    } else {
        Ok(())
    }
}

/// Parse and discard a Firefox bulk frame.
///
/// Called when `recv_from` sees a leading `b` (already consumed).  Delegates
/// to [`drain_bulk_frame_with_cap`] for the shared drain logic, then maps the result to
/// [`ProtocolError::BulkPacketUnsupported`] so the caller can log and skip.
///
/// Returns [`ProtocolError::BulkPacketUnsupported`] on success (body skipped)
/// or a parse/IO error if the stream is malformed.
fn recv_bulk_frame<R: BufRead>(
    reader: &mut R,
    first_byte: u8,
    cap: usize,
) -> Result<Value, ProtocolError> {
    let (actor, kind, length) = drain_bulk_frame_with_cap(reader, first_byte, cap)?;
    Err(ProtocolError::BulkPacketUnsupported {
        actor,
        kind,
        length,
    })
}

// ---------------------------------------------------------------------------
// I/O error mapping helpers
// ---------------------------------------------------------------------------

fn map_recv_io_error(e: std::io::Error) -> ProtocolError {
    if e.kind() == std::io::ErrorKind::TimedOut || e.kind() == std::io::ErrorKind::WouldBlock {
        ProtocolError::Timeout
    } else {
        ProtocolError::RecvFailed(e)
    }
}

fn map_send_io_error(e: std::io::Error) -> ProtocolError {
    if e.kind() == std::io::ErrorKind::TimedOut || e.kind() == std::io::ErrorKind::WouldBlock {
        ProtocolError::Timeout
    } else {
        ProtocolError::SendFailed(e)
    }
}

/// Test-only RAII guard for the **only** sanctioned mutation of
/// `MAX_FRAME_BYTES_CELL` inside test builds: *raising* the cap.  Restores the
/// previous raw cell value on drop, including on panic unwind.
///
/// # Why tests may not shrink the process-global cap
///
/// `cargo test` runs every test in the crate's unit-test binary on a shared
/// thread pool, so the cap is genuinely shared mutable state across `#[test]`
/// fns in *different* modules — a mutation by `transport::tests` is visible to
/// a `specs::types` test parsing a 20 KB frame at that instant.
///
/// iter-150 tried to police that with `FRAME_CAP_LOCK`, an `RwLock<()>` whose
/// contract was "shrink the cap only under a `write` guard; parse a frame that
/// could exceed a shrunk cap only under a `read` guard".  The write side was
/// implemented at all five shrinking tests and the read side essentially never
/// was: iter-196 found *one* `read()` call in the whole workspace, and four
/// unguarded 15–20 KB readers besides
/// (`actors::page_style::tests::parse_computed_properties_resolves_longstring_value`,
/// `actors::dom_walker::tests::parse_dom_node_resolves_longstring_node_value`,
/// `actors::dom_walker::tests::parse_dom_node_resolves_longstring_attr_value`,
/// `actors::storage::tests::parse_cookie_resolves_longstring_value`).  A
/// reader-less `RwLock` is indistinguishable from a `Mutex` between writers,
/// which is exactly why the discipline looked complete while
/// `transport::tests::recv_bulk_with_handler_chunked` (20 KB body) kept losing
/// the race and failing with `BulkFrameTooLarge { announced: 20000, max: 1024 }`
/// on branches that touch no transport code at all.
///
/// The fix is structural, not conventional: every frame parser now has a
/// `*_with_cap` form ([`recv_from_with_cap`], [`drain_bulk_frame_with_cap`],
/// [`check_outbound_bulk_size`]), so a test that wants a small cap passes one
/// instead of shrinking the cell.  No test shrinks the cap, so no test needs a
/// read guard, and the lock is gone.
///
/// Raising is still allowed — it is the one direction that cannot make a
/// concurrent parse *stricter*, so no unsynchronised reader can be broken by
/// it — and [`raise_to`](RaisedFrameCap::raise_to) panics on any attempt to go
/// below [`DEFAULT_MAX_FRAME_BYTES`], which is what makes "tests do not shrink
/// the cap" an enforced property rather than a comment.  Pinned by
/// `transport::tests::raised_frame_cap_refuses_to_shrink`.
#[cfg(test)]
pub(crate) struct RaisedFrameCap {
    /// Raw cell value observed at construction — restored verbatim on drop, so
    /// an unset (`0`) cell stays unset.
    prev: usize,
    /// Held for the guard's lifetime so two raisers cannot interleave their
    /// snapshot/restore pairs and leak a raised cap.  Readers need no guard:
    /// a raised cap cannot make any parse stricter.
    _exclusive: std::sync::MutexGuard<'static, ()>,
}

/// Excludes concurrent [`RaisedFrameCap`] holders from each other.  Not a
/// reader/writer lock — nothing reads under it — so there is no unimplemented
/// half this time.
#[cfg(test)]
static FRAME_CAP_RAISE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
impl RaisedFrameCap {
    /// Raise the process-global cap to `bytes`.
    ///
    /// # Panics
    ///
    /// If `bytes` is below [`DEFAULT_MAX_FRAME_BYTES`].  Shrinking the shared
    /// cell is what made `cargo test --workspace` intermittently red; use the
    /// `*_with_cap` parsers instead.
    pub(crate) fn raise_to(bytes: usize) -> Self {
        // Asserted *before* the lock is taken so the rejection path cannot
        // poison it for well-behaved callers.
        assert!(
            bytes >= DEFAULT_MAX_FRAME_BYTES,
            "tests must not shrink the process-global frame cap ({bytes} < {DEFAULT_MAX_FRAME_BYTES}); \
             pass the cap explicitly via recv_from_with_cap / drain_bulk_frame_with_cap instead"
        );
        let exclusive = FRAME_CAP_RAISE_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let prev = MAX_FRAME_BYTES_CELL.load(Ordering::Relaxed);
        MAX_FRAME_BYTES_CELL.store(bytes, Ordering::Relaxed);
        Self {
            prev,
            _exclusive: exclusive,
        }
    }
}

#[cfg(test)]
impl Drop for RaisedFrameCap {
    fn drop(&mut self) {
        MAX_FRAME_BYTES_CELL.store(self.prev, Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    /// Serialize access to the `set_trace_raw_for_test` override so that tests
    /// manipulating redaction state don't race with each other.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Module-level lock shared by every test that mutates the global
    /// `REDACT_THRESHOLD`.  Combined with [`RedactThresholdGuard`] this
    /// guarantees both serialization and panic-safe restoration.
    static REDACT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// RAII guard that snapshots the current redaction threshold on
    /// construction and restores it on drop, even if the test panics.
    struct RedactThresholdGuard {
        prev: usize,
    }

    impl RedactThresholdGuard {
        fn new() -> Self {
            Self {
                prev: REDACT_THRESHOLD.load(Ordering::Relaxed),
            }
        }
    }

    impl Drop for RedactThresholdGuard {
        fn drop(&mut self) {
            REDACT_THRESHOLD.store(self.prev, Ordering::Relaxed);
        }
    }

    // -----------------------------------------------------------------------
    // encode_frame — pure, no I/O
    // -----------------------------------------------------------------------

    #[test]
    fn encode_produces_correct_length_prefix() {
        let json = r#"{"type":"listTabs","to":"root"}"#;
        let frame = encode_frame(json);
        let expected = format!("{}:{}", json.len(), json);
        assert_eq!(frame, expected);
    }

    #[test]
    fn encode_length_matches_byte_count() {
        let json = r#"{"v":"héllo"}"#; // multi-byte UTF-8
        let frame = encode_frame(json);
        let colon = frame.find(':').unwrap();
        let declared: usize = frame[..colon].parse().unwrap();
        assert_eq!(declared, json.len());
    }

    // -----------------------------------------------------------------------
    // recv_from — uses Cursor<&[u8]> instead of a live socket
    // -----------------------------------------------------------------------

    #[test]
    fn recv_parses_valid_frame() {
        let payload = r#"{"type":"listTabs","to":"root"}"#;
        let frame = encode_frame(payload);
        let mut cursor = Cursor::new(frame.into_bytes());

        let value = recv_from(&mut cursor).unwrap();
        assert_eq!(value["type"], "listTabs");
        assert_eq!(value["to"], "root");
    }

    #[test]
    fn recv_handles_multi_digit_length() {
        let long_value: String = "x".repeat(200);
        let payload = serde_json::to_string(&serde_json::json!({"v": long_value})).unwrap();
        assert!(payload.len() >= 100, "payload must have a 3-digit length");

        let frame = encode_frame(&payload);
        let mut cursor = Cursor::new(frame.into_bytes());

        let value = recv_from(&mut cursor).unwrap();
        assert_eq!(value["v"].as_str().unwrap(), long_value);
    }

    #[test]
    fn recv_errors_on_non_digit_in_length_prefix() {
        let bad = b"x:{}";
        let mut cursor = Cursor::new(bad.as_ref());

        let err = recv_from(&mut cursor).unwrap_err();
        assert!(
            matches!(err, ProtocolError::InvalidPacket(_)),
            "expected InvalidPacket, got {err:?}"
        );
    }

    #[test]
    fn recv_errors_on_empty_length_prefix() {
        let bad = b":{}";
        let mut cursor = Cursor::new(bad.as_ref());

        let err = recv_from(&mut cursor).unwrap_err();
        assert!(
            matches!(err, ProtocolError::InvalidPacket(_)),
            "expected InvalidPacket, got {err:?}"
        );
    }

    #[test]
    fn recv_errors_on_invalid_json_body() {
        let bad_body = b"not-json";
        let frame = format!("{}:{}", bad_body.len(), String::from_utf8_lossy(bad_body));
        let mut cursor = Cursor::new(frame.into_bytes());

        let err = recv_from(&mut cursor).unwrap_err();
        assert!(
            matches!(err, ProtocolError::InvalidPacket(_)),
            "expected InvalidPacket, got {err:?}"
        );
    }

    #[test]
    fn recv_errors_on_length_prefix_too_long() {
        // 20 consecutive digit bytes with no colon triggers the >= 20 guard.
        let frame = "1".repeat(20);
        let mut cursor = Cursor::new(frame.into_bytes());

        let err = recv_from(&mut cursor).unwrap_err();
        assert!(
            matches!(err, ProtocolError::InvalidPacket(_)),
            "expected InvalidPacket, got {err:?}"
        );
    }

    #[test]
    fn recv_rejects_frame_exceeding_max_size() {
        // Declare a 400 MB frame (> default 256 MiB cap).  No allocation
        // should happen — the error must be returned before reading the body.
        // We only send the length prefix followed by a colon; the cursor has
        // no body bytes, so if recv_from tried to allocate and read we would
        // get a RecvFailed instead of FrameTooLarge.
        //
        // iter-196: the cap is passed explicitly rather than read from the
        // process-global cell, so the `max` field can be asserted exactly (a
        // sibling test raising the cell would otherwise change it) and so this
        // test never depends on shared mutable state.
        let declared = 400_000_000usize;
        let prefix = format!("{declared}:");
        let mut cursor = Cursor::new(prefix.into_bytes());

        let err = recv_from_with_cap(&mut cursor, DEFAULT_MAX_FRAME_BYTES).unwrap_err();
        assert!(
            matches!(
                err,
                ProtocolError::FrameTooLarge {
                    declared: 400_000_000,
                    max: DEFAULT_MAX_FRAME_BYTES
                }
            ),
            "expected FrameTooLarge, got {err:?}"
        );
    }

    /// AC: `max_frame_mb_knob_works`.  A lower cap rejects a frame that a
    /// higher cap admits, and the `--max-frame-mb` value the CLI stores is the
    /// one `recv_from` actually enforces.
    ///
    /// iter-196: the first two thirds of this test used to shrink the
    /// process-global cap to 1024 bytes under a `FRAME_CAP_LOCK` write guard,
    /// which is what made unrelated 20 KB tests fail intermittently.  Both are
    /// now expressed with an explicit cap, so nothing shared is mutated.  Only
    /// the last third touches the global cell, and only to *raise* it — see
    /// [`RaisedFrameCap`].
    #[test]
    fn max_frame_mb_knob_works() {
        // The `0 means unset` rule, without touching the global cell.
        assert_eq!(resolve_frame_cap(0), DEFAULT_MAX_FRAME_BYTES);
        assert_eq!(resolve_frame_cap(1024), 1024);

        // A 1024-byte cap rejects a 2000-byte frame …
        let mut cursor = Cursor::new(b"2000:".to_vec());
        let err = recv_from_with_cap(&mut cursor, 1024).unwrap_err();
        assert!(
            matches!(
                err,
                ProtocolError::FrameTooLarge {
                    declared: 2000,
                    max: 1024
                }
            ),
            "expected FrameTooLarge {{declared:2000, max:1024}}, got {err:?}"
        );

        // … and a 4096-byte cap lets the same length past the size check (it
        // then fails at body read since the cursor holds no body, which is
        // fine — we only care that the FrameTooLarge branch did NOT fire).
        let mut cursor = Cursor::new(b"2000:".to_vec());
        let err = recv_from_with_cap(&mut cursor, 4096).unwrap_err();
        assert!(
            !matches!(err, ProtocolError::FrameTooLarge { .. }),
            "raising the cap must allow the frame past the size check, got {err:?}"
        );

        // The knob itself: what `set_max_frame_bytes` stores is what the
        // global-reading entry points enforce.  A bulk header is used rather
        // than a JSON frame so passing the cap check costs no allocation —
        // `drain_bulk_body` streams in 8 KiB chunks and hits EOF immediately.
        let raised = DEFAULT_MAX_FRAME_BYTES * 2;
        let _raise = RaisedFrameCap::raise_to(raised);
        assert_eq!(max_frame_bytes(), raised, "the knob must round-trip");

        let announced = DEFAULT_MAX_FRAME_BYTES as u64 + 1;
        let mut cursor = Cursor::new(format!("bulk a1 k1 {announced}:").into_bytes());
        let err = recv_from(&mut cursor).unwrap_err();
        assert!(
            !matches!(err, ProtocolError::BulkFrameTooLarge { .. }),
            "a length under the raised global cap must pass the cap check, got {err:?}"
        );

        // RaisedFrameCap restores the previous raw value on drop.
    }

    /// AC (iter-196) `frame_cap_cannot_be_shrunk_by_tests`: the replacement for
    /// iter-150's `FRAME_CAP_LOCK` self-test.
    ///
    /// The invariant that survives the lock's removal is stronger and cheaper
    /// to hold than "every cap reader remembers to take a read guard" (which
    /// no reader outside `transport.rs` ever did): tests may raise the
    /// process-global cap but never shrink it, so no unsynchronised reader can
    /// observe a cap smaller than the default it was written against.  This
    /// pins the enforcement — without it `raise_to` is just a comment.
    #[test]
    #[should_panic(expected = "must not shrink the process-global frame cap")]
    fn raised_frame_cap_refuses_to_shrink() {
        let _guard = RaisedFrameCap::raise_to(1024);
    }

    /// A raise is undone on drop, so the cap a later test observes is whatever
    /// it was before — the property that makes the raise invisible to
    /// concurrent readers once the guard is gone.
    ///
    /// Every observation of the cell here is taken *under* the raise lock, via
    /// `RaisedFrameCap::prev`. A bare `MAX_FRAME_BYTES_CELL.load()` before the
    /// guard is constructed races with a sibling raiser and made this very test
    /// fail 35 times in 200 runs at `--test-threads=16` — the same shape of
    /// mistake this iteration is removing, caught by the same stress loop.
    #[test]
    fn raised_frame_cap_restores_previous_value_on_drop() {
        let before = {
            let raise = RaisedFrameCap::raise_to(DEFAULT_MAX_FRAME_BYTES * 3);
            assert_eq!(max_frame_bytes(), DEFAULT_MAX_FRAME_BYTES * 3);
            raise.prev
        };

        // The next raiser sees what the dropped guard put back. Any raiser that
        // slipped in between restored the same value on its own drop, so this
        // is a fact about the guard rather than a race.
        let after = RaisedFrameCap::raise_to(DEFAULT_MAX_FRAME_BYTES * 4);
        assert_eq!(
            after.prev, before,
            "the raw cell value must be restored verbatim, unset included"
        );
    }

    /// AC: `redact_threshold_tunable`.  A long non-sensitive string passes
    /// through after raising the threshold; sensitive-keyed values still
    /// redact regardless.
    #[test]
    fn redact_threshold_tunable() {
        // Serialise + restore on panic. Also hold ENV_LOCK: `redact()` reads
        // the shared trace-raw override, which `redact_noop_when_ff_rdp_trace_raw_set`
        // mutates under that lock — without it this test can race and see raw
        // (non-redacted) output.
        let _env_g = ENV_LOCK.lock().unwrap();
        let _g = REDACT_LOCK.lock().unwrap();
        let _restore = RedactThresholdGuard::new();

        let long_url =
            "https://example.com/path?utm_source=newsletter&utm_campaign=spring&q=very+long+search";
        assert!(long_url.len() > 64);

        // With a generous threshold, the URL renders in full.
        set_redact_threshold(512);
        let v = serde_json::json!({"url": long_url, "authorization": "Bearer abc"});
        let r = redact(&v);
        assert_eq!(
            r["url"].as_str().unwrap(),
            long_url,
            "long URL must pass through when threshold > url.len()"
        );
        let auth = r["authorization"].as_str().unwrap();
        assert!(
            auth.starts_with("<redacted"),
            "sensitive key must still redact regardless of threshold: {auth}"
        );

        // With a tight threshold, the same URL is redacted.
        set_redact_threshold(16);
        let r2 = redact(&v);
        let url2 = r2["url"].as_str().unwrap();
        assert!(
            url2.starts_with("<redacted"),
            "tight threshold must redact long URL: {url2}"
        );

        // RedactThresholdGuard restores the previous value on drop.
    }

    // -----------------------------------------------------------------------
    // redact — pure unit tests
    // -----------------------------------------------------------------------

    #[test]
    fn redact_sensitive_key_replaces_value() {
        // Hold ENV_LOCK: `redact()` reads the shared trace-raw override, which
        // `redact_noop_when_ff_rdp_trace_raw_set` mutates under that lock —
        // without it this test can race and see raw (non-redacted) output.
        let _env_g = ENV_LOCK.lock().unwrap();
        let v = serde_json::json!({"cookie": "session=abc123"});
        let r = redact(&v);
        let s = r["cookie"].as_str().unwrap();
        assert!(s.starts_with("<redacted"), "expected redaction, got: {s}");
    }

    #[test]
    fn redact_source_key_replaces_value() {
        let _env_g = ENV_LOCK.lock().unwrap();
        let v = serde_json::json!({"text": "console.log('hello')"});
        let r = redact(&v);
        let s = r["text"].as_str().unwrap();
        assert!(s.starts_with("<redacted"), "expected redaction, got: {s}");
    }

    #[test]
    fn redact_long_string_replaces_value() {
        // Serialise with other tests that mutate REDACT_THRESHOLD so that
        // the read+redact pair sees a stable cap. Also holds ENV_LOCK for the
        // same trace-raw-override race described on redact_sensitive_key_replaces_value.
        let _env_g = ENV_LOCK.lock().unwrap();
        let _g = REDACT_LOCK.lock().unwrap();
        let long = "x".repeat(redact_threshold() + 1);
        let v = serde_json::json!({"data": long});
        let r = redact(&v);
        let s = r["data"].as_str().unwrap();
        assert!(
            s.starts_with("<redacted"),
            "long string should be redacted, got: {s}"
        );
    }

    #[test]
    fn redact_short_string_passes_through() {
        let _env_g = ENV_LOCK.lock().unwrap();
        let short = "short";
        let v = serde_json::json!({"data": short});
        let r = redact(&v);
        assert_eq!(r["data"].as_str().unwrap(), short);
    }

    #[test]
    fn redact_noop_when_ff_rdp_trace_raw_set() {
        // Use the test override rather than mutating the process environment.
        // Lock ENV_LOCK to prevent races between tests that touch this state.
        let _guard = ENV_LOCK.lock().unwrap();
        set_trace_raw_for_test(Some(true));

        let secret = "a".repeat(100);
        let v = serde_json::json!({"cookie": secret.clone()});
        let r = redact(&v);
        // Raw mode: no redaction.
        assert_eq!(r["cookie"].as_str().unwrap(), secret);

        // Restore: clear the override so other tests see the default behaviour.
        set_trace_raw_for_test(None);
    }

    #[test]
    fn redact_nested_object_handles_sensitive_key() {
        let _env_g = ENV_LOCK.lock().unwrap();
        let v =
            serde_json::json!({"headers": {"cookie": "session=abc", "content-type": "text/html"}});
        let r = redact(&v);
        let cookie = r["headers"]["cookie"].as_str().unwrap();
        assert!(
            cookie.starts_with("<redacted"),
            "cookie in nested obj must be redacted"
        );
        // Non-sensitive key at same level passes through.
        assert_eq!(r["headers"]["content-type"].as_str().unwrap(), "text/html");
    }

    // -----------------------------------------------------------------------
    // send via RdpTransport — minimal loopback test
    // -----------------------------------------------------------------------

    #[test]
    fn send_produces_correct_frame_over_socket() {
        use std::io::Read;
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        // Connect client before accepting so the handshake completes.
        let client_stream = TcpStream::connect(addr).unwrap();
        let (server_stream, _) = listener.accept().unwrap();

        let writer = client_stream.try_clone().unwrap();
        let reader = BufReader::new(client_stream);
        let mut transport = RdpTransport {
            reader,
            writer,
            event_sink: None,
            target_guard: None,
            navigation_started: None,
        };

        let msg = serde_json::json!({"type": "listTabs", "to": "root"});
        transport.send(&msg).unwrap();

        // Drop the transport so the server sees EOF.
        drop(transport);

        let mut buf = Vec::new();
        let mut srv_reader = server_stream;
        srv_reader.read_to_end(&mut buf).unwrap();

        let raw = String::from_utf8(buf).unwrap();
        let expected_json = serde_json::to_string(&msg).unwrap();
        assert_eq!(raw, encode_frame(&expected_json));
    }

    // -----------------------------------------------------------------------
    // Bulk frame handling (Theme C, iter-61w)
    // -----------------------------------------------------------------------

    /// Build a synthetic bulk frame: `bulk <actor> <kind> <length>:<body>`.
    fn make_bulk_frame(actor: &str, kind: &str, body: &[u8]) -> Vec<u8> {
        let header = format!("bulk {} {} {}:", actor, kind, body.len());
        let mut bytes = header.into_bytes();
        bytes.extend_from_slice(body);
        bytes
    }

    #[test]
    fn bulk_frame_returns_bulk_packet_unsupported() {
        let frame = make_bulk_frame("conn0/actor1", "screenshot", b"binary payload");
        let mut cursor = Cursor::new(frame);

        let err = recv_from(&mut cursor).unwrap_err();
        assert!(
            matches!(
                err,
                ProtocolError::BulkPacketUnsupported {
                    actor: ref a,
                    kind: ref k,
                    length: 14,
                } if a == "conn0/actor1" && k == "screenshot"
            ),
            "expected BulkPacketUnsupported with correct fields, got: {err:?}"
        );
    }

    #[test]
    fn bulk_frame_followed_by_json_frame_parses_correctly() {
        // Simulate a stream with a bulk frame followed by a normal JSON packet.
        let bulk = make_bulk_frame("conn0/actor1", "blob", b"some binary data");
        let json_payload = r#"{"type":"continue","from":"root"}"#;
        let json_frame = encode_frame(json_payload);

        let mut stream: Vec<u8> = bulk;
        stream.extend_from_slice(json_frame.as_bytes());

        let mut cursor = Cursor::new(stream);

        // First recv: bulk — should return error but consume the body.
        let err = recv_from(&mut cursor).unwrap_err();
        assert!(
            matches!(err, ProtocolError::BulkPacketUnsupported { .. }),
            "first recv should be BulkPacketUnsupported, got: {err:?}"
        );

        // Second recv: the JSON packet must parse correctly after the skip.
        let value = recv_from(&mut cursor).unwrap();
        assert_eq!(value["type"], "continue");
        assert_eq!(value["from"], "root");
    }

    /// AC: `recv_bulk_frame` must reject a body that exceeds the configured
    /// cap **before** allocating or reading the body — proven here by giving
    /// the cursor only the header bytes.  If the implementation tried to
    /// stream the body we would observe an EOF / IO error instead of
    /// `BulkFrameTooLarge`.
    #[test]
    fn bulk_frame_rejects_oversized_announcement() {
        // Header only — no body bytes — declared length way above the cap.
        // If `recv_bulk_frame` allocated/read the body we would observe an
        // EOF instead of `BulkFrameTooLarge`.
        let header = b"bulk conn0/actor1 heap 8000000:";
        let mut cursor = Cursor::new(header.to_vec());

        let err = recv_from_with_cap(&mut cursor, 1024).unwrap_err();
        assert!(
            matches!(
                err,
                ProtocolError::BulkFrameTooLarge {
                    announced: 8_000_000,
                    max: 1024
                }
            ),
            "expected BulkFrameTooLarge {{announced:8_000_000, max:1024}}, got {err:?}"
        );
    }

    /// AC: `bulk_frame_cap_send_side` — the outbound size guard refuses to
    /// promise a frame larger than our own receive cap.  Catches local bugs
    /// before the wire commits.
    #[test]
    fn bulk_frame_cap_send_side() {
        let err = check_outbound_bulk_size(2048, 1024).unwrap_err();
        assert!(
            matches!(
                err,
                ProtocolError::BulkFrameTooLarge {
                    announced: 2048,
                    max: 1024
                }
            ),
            "send-side cap must reject oversize length, got {err:?}"
        );

        // At-cap is fine; below-cap is fine.
        check_outbound_bulk_size(1024, 1024).unwrap();
        check_outbound_bulk_size(0, 1024).unwrap();
    }

    /// AC: `transport_rejects_deep_json` — a 200-level nested JSON object must
    /// return an error (serde_json hits its recursion limit at 128) without
    /// panicking or causing a stack overflow.
    #[test]
    fn transport_rejects_deep_json() {
        // Build a 200-level deep nested JSON: `{"a":{"a":{...}}}`.
        let depth = 200;
        let mut payload = String::with_capacity(depth * 6 + 10);
        for _ in 0..depth {
            payload.push_str("{\"a\":");
        }
        payload.push_str("null");
        for _ in 0..depth {
            payload.push('}');
        }
        // This 1204-byte frame goes through the process-global cap, and until
        // iter-196 that assumption was unfounded: four sibling tests shrank the
        // cap to 1024 bytes, so this test could observe `FrameTooLarge` instead
        // of `InvalidPacket`. Nothing shrinks the cap now — see
        // [`RaisedFrameCap`] — so the default 256 MiB floor holds.
        let frame = encode_frame(&payload);
        let mut cursor = Cursor::new(frame.into_bytes());

        let err = recv_from(&mut cursor).unwrap_err();
        assert!(
            matches!(err, ProtocolError::InvalidPacket(_)),
            "deeply nested JSON must surface as InvalidPacket (serde_json depth limit), got {err:?}"
        );
    }

    #[test]
    fn bulk_frame_empty_body_is_handled() {
        let frame = make_bulk_frame("conn0/blob1", "empty", b"");
        let mut cursor = Cursor::new(frame);

        let err = recv_from(&mut cursor).unwrap_err();
        assert!(
            matches!(err, ProtocolError::BulkPacketUnsupported { length: 0, .. }),
            "expected BulkPacketUnsupported with length 0, got: {err:?}"
        );
    }

    // -----------------------------------------------------------------------
    // recv_reply_from / recv_event_from (iter-69)
    // -----------------------------------------------------------------------

    use std::io::Write as IoWrite;
    use std::net::TcpListener;

    fn make_transport_pair() -> (RdpTransport, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let client = TcpStream::connect(addr).unwrap();
        let (server_stream, _) = listener.accept().unwrap();
        let writer = client.try_clone().unwrap();
        let reader = BufReader::new(client);
        (RdpTransport::from_parts(reader, writer), server_stream)
    }

    fn write_frame(stream: &TcpStream, msg: &Value) {
        let json = serde_json::to_string(msg).unwrap();
        IoWrite::write_all(
            &mut stream.try_clone().unwrap(),
            encode_frame(&json).as_bytes(),
        )
        .unwrap();
    }

    /// AC: `actor_request_routes_event_correctly` — a `consoleAPICall` from the
    /// target actor arrives first; the reply (no `type`) arrives second. The
    /// reply must be returned and the event must land on the event sink.
    #[test]
    fn recv_reply_from_routes_event_to_sink() {
        let (mut transport, server) = make_transport_pair();
        let (tx, rx) = std::sync::mpsc::channel::<Value>();
        transport.set_event_sink(Some(tx));

        let server_thread = std::thread::spawn(move || {
            // First: a push event with the right `from` (the bug we are fixing
            // misclassified this as the reply).
            write_frame(
                &server,
                &serde_json::json!({
                    "from": "actorA",
                    "type": "consoleAPICall",
                    "message": {"level": "log", "arguments": ["noise"]}
                }),
            );
            // Second: the actual reply — same `from`, no `type`.
            write_frame(
                &server,
                &serde_json::json!({"from": "actorA", "result": 42}),
            );
        });

        let reply = recv_reply_from(&mut transport, "actorA").unwrap();
        assert_eq!(reply["result"], 42);
        assert!(reply.get("type").is_none(), "reply must not have a type");

        let event = rx
            .try_recv()
            .expect("the misclassified event should be on the sink");
        assert_eq!(event["type"], "consoleAPICall");

        server_thread.join().unwrap();
    }

    /// iter-121: `swap_event_sink` installs a new sink and returns the previous
    /// one, so a caller can temporarily capture push events (e.g. the
    /// `resources-available-array` that precedes a `watchResources` ACK on
    /// FF152) and then restore whatever sink was there before — without
    /// clobbering a daemon-installed one.
    #[test]
    fn swap_event_sink_returns_previous_and_installs_new() {
        let (mut transport, _server) = make_transport_pair();

        // No sink initially → swap returns None and installs sink A.
        let (tx_a, rx_a) = std::sync::mpsc::channel::<Value>();
        let prev = transport.swap_event_sink(Some(tx_a));
        assert!(prev.is_none(), "no sink was installed initially");

        // Swap in sink B, recover sink A.
        let (tx_b, _rx_b) = std::sync::mpsc::channel::<Value>();
        let restored_a = transport
            .swap_event_sink(Some(tx_b))
            .expect("swap must return the previously-installed sink A");

        // The returned sender must be the original A: routing an event to it
        // arrives on rx_a.
        transport.forward_event(serde_json::json!({"type": "probe", "from": "x"}));
        // Sink B is now active, so rx_a should NOT receive the probe.
        assert!(
            rx_a.try_recv().is_err(),
            "sink A is no longer active after swapping in B"
        );
        // But the returned handle is A: sending through it reaches rx_a.
        restored_a
            .send(serde_json::json!({"marker": true}))
            .unwrap();
        assert_eq!(rx_a.try_recv().unwrap()["marker"], true);
    }

    /// AC: `actor_request_rejects_typed_packet_as_reply` — a typed packet
    /// (e.g. `paused`) must NOT be returned even if `from == actor`.
    #[test]
    fn recv_reply_from_rejects_typed_packet_as_reply() {
        let (mut transport, server) = make_transport_pair();

        let server_thread = std::thread::spawn(move || {
            // ThreadActor pseudo-`paused` event with the same `from`.
            write_frame(
                &server,
                &serde_json::json!({"from": "thread1", "type": "paused", "why": {"type": "attached"}}),
            );
            // The real reply.
            write_frame(
                &server,
                &serde_json::json!({"from": "thread1", "actor": "thread1"}),
            );
        });

        let reply = recv_reply_from(&mut transport, "thread1").unwrap();
        assert!(reply.get("type").is_none());
        assert_eq!(reply["actor"], "thread1");

        server_thread.join().unwrap();
    }

    /// `recv_reply_from` must forward sibling-actor packets to the event sink
    /// (iter-74: they must not be silently dropped).
    ///
    /// AC: `recv_reply_from_forwards_sibling_packet`
    #[test]
    fn recv_reply_from_forwards_sibling_packet() {
        let (mut transport, server) = make_transport_pair();
        let (tx, rx) = std::sync::mpsc::channel::<Value>();
        transport.set_event_sink(Some(tx));

        let server_thread = std::thread::spawn(move || {
            // Sibling-actor event that arrives while we await actorA's reply.
            write_frame(
                &server,
                &serde_json::json!({"from": "otherActor", "type": "tabListChanged"}),
            );
            // The real reply from actorA.
            write_frame(&server, &serde_json::json!({"from": "actorA", "ok": true}));
        });

        let reply = recv_reply_from(&mut transport, "actorA").unwrap();
        assert_eq!(reply["ok"], true);

        // The sibling packet must have been forwarded to the event sink.
        let sibling = rx
            .try_recv()
            .expect("sibling packet must be forwarded to the event sink");
        assert_eq!(sibling["type"], "tabListChanged");
        server_thread.join().unwrap();
    }

    /// `recv_reply_from` must surface actor `error` packets as
    /// `ProtocolError::ActorError` with the typed kind.
    #[test]
    fn recv_reply_from_maps_error_packet() {
        let (mut transport, server) = make_transport_pair();

        let server_thread = std::thread::spawn(move || {
            write_frame(
                &server,
                &serde_json::json!({
                    "from": "actorA",
                    "error": "missingParameter",
                    "message": "required field 'url'"
                }),
            );
        });

        let err = recv_reply_from(&mut transport, "actorA").unwrap_err();
        match err {
            ProtocolError::ActorError { kind, message, .. } => {
                assert_eq!(kind, ActorErrorKind::MissingParameter);
                assert!(message.contains("required field 'url'"));
            }
            other => panic!("expected ActorError, got {other:?}"),
        }
        server_thread.join().unwrap();
    }

    /// iter-101 Theme B: a `daemon_busy` control-error frame (`from == "daemon"`)
    /// arriving while awaiting an actor reply must surface promptly as an
    /// `ActorError` rather than being forwarded as a sibling event and hanging
    /// until the socket timeout.
    #[test]
    fn recv_reply_from_surfaces_daemon_busy_control_error() {
        let (mut transport, server) = make_transport_pair();
        let (tx, rx) = std::sync::mpsc::channel::<Value>();
        transport.set_event_sink(Some(tx));

        let server_thread = std::thread::spawn(move || {
            write_frame(
                &server,
                &serde_json::json!({
                    "from": "daemon",
                    "error": "daemon_busy",
                    "message": "another CLI client is holding the daemon's RPC channel"
                }),
            );
        });

        let err = recv_reply_from(&mut transport, "actorA").unwrap_err();
        match err {
            ProtocolError::ActorError {
                actor,
                error,
                message,
                ..
            } => {
                assert_eq!(actor, "daemon");
                assert_eq!(error, "daemon_busy");
                assert!(message.contains("RPC channel"));
            }
            other => panic!("expected ActorError from daemon, got {other:?}"),
        }
        // The control-error frame must NOT be forwarded to the event sink — it
        // is terminal, not a stray event.
        assert!(
            rx.try_recv().is_err(),
            "daemon control-error must not leak to the event sink"
        );
        server_thread.join().unwrap();
    }

    /// `recv_event_from` must surface an error reply from the target actor
    /// instead of silently skipping it — otherwise callers like
    /// `ThreadActor::attach` would hang until the socket timeout.
    #[test]
    fn recv_event_from_surfaces_error_reply() {
        let (mut transport, server) = make_transport_pair();

        let server_thread = std::thread::spawn(move || {
            write_frame(
                &server,
                &serde_json::json!({
                    "from": "thread1",
                    "error": "wrongState",
                    "message": "thread already attached"
                }),
            );
        });

        let err = recv_event_from(&mut transport, "thread1", |m| {
            m.get("type").and_then(Value::as_str) == Some("paused")
        })
        .unwrap_err();
        match err {
            ProtocolError::ActorError { kind, message, .. } => {
                assert_eq!(kind, ActorErrorKind::WrongState);
                assert!(message.contains("already attached"));
            }
            other => panic!("expected ActorError, got {other:?}"),
        }
        server_thread.join().unwrap();
    }

    /// `recv_event_from` matches the first packet that satisfies the predicate.
    #[test]
    fn recv_event_from_matches_predicate() {
        let (mut transport, server) = make_transport_pair();

        let server_thread = std::thread::spawn(move || {
            write_frame(
                &server,
                &serde_json::json!({"from": "actorA", "type": "consoleAPICall"}),
            );
            write_frame(
                &server,
                &serde_json::json!({"from": "actorA", "type": "evaluationResult", "resultID": "x"}),
            );
        });

        let msg = recv_event_from(&mut transport, "actorA", |m| {
            m.get("type").and_then(Value::as_str) == Some("evaluationResult")
        })
        .unwrap();
        assert_eq!(msg["resultID"], "x");
        server_thread.join().unwrap();
    }

    /// AC: `recv_event_from_forwards_non_matching` — intermediate non-matching
    /// events from the target actor (e.g. `consoleAPICall` while awaiting
    /// `evaluationResult`) must be forwarded to the event sink, not dropped.
    ///
    /// Simulates the `evaluateJSAsync` sequence from
    /// `devtools/server/actors/webconsole.js:761-870` where the console actor
    /// emits `consoleAPICall` before the final `evaluationResult`.
    #[test]
    fn recv_event_from_forwards_non_matching() {
        let (mut transport, server) = make_transport_pair();
        let (tx, rx) = std::sync::mpsc::channel::<Value>();
        transport.set_event_sink(Some(tx));

        let server_thread = std::thread::spawn(move || {
            // Intermediate console event (non-matching) — must reach the sink.
            write_frame(
                &server,
                &serde_json::json!({
                    "from": "consoleActor",
                    "type": "consoleAPICall",
                    "message": {"level": "log", "arguments": ["ping"]}
                }),
            );
            // Also a sibling event from a different actor.
            write_frame(
                &server,
                &serde_json::json!({"from": "watcherActor", "type": "target-available-form"}),
            );
            // The matching event.
            write_frame(
                &server,
                &serde_json::json!({
                    "from": "consoleActor",
                    "type": "evaluationResult",
                    "resultID": "r1",
                    "result": 2
                }),
            );
        });

        let result = recv_event_from(&mut transport, "consoleActor", |m| {
            m.get("type").and_then(Value::as_str) == Some("evaluationResult")
        })
        .unwrap();
        assert_eq!(result["result"], 2);

        // The consoleAPICall (non-matching from target actor) must be on the sink.
        let forwarded: Vec<Value> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
        assert_eq!(
            forwarded.len(),
            2,
            "expected 2 forwarded packets (consoleAPICall + target-available-form), got {}",
            forwarded.len()
        );
        assert_eq!(forwarded[0]["type"], "consoleAPICall");
        assert_eq!(forwarded[1]["type"], "target-available-form");

        server_thread.join().unwrap();
    }

    // -----------------------------------------------------------------------
    // recv_bulk_with_handler (Theme A, iter-76)
    // -----------------------------------------------------------------------

    /// AC: `recv_bulk_with_handler_chunked` — confirms that the handler copies
    /// body bytes in chunks without buffering the full body in memory, and
    /// returns the correct byte count.
    #[test]
    fn recv_bulk_with_handler_chunked() {
        // Build a synthetic bulk frame whose body is larger than one chunk.
        let body: Vec<u8> = (0u8..=255).cycle().take(20_000).collect(); // > 8 KiB
        let frame = make_bulk_frame("conn0/heapSnap1", "bulkData", &body);
        let mut cursor = Cursor::new(frame);

        let mut out = Vec::new();
        let bytes_written =
            recv_bulk_with_handler_from(&mut cursor, "conn0/heapSnap1", "bulkData", &mut out)
                .unwrap();

        assert_eq!(bytes_written, 20_000);
        assert_eq!(out, body, "output must match the raw body byte-for-byte");
    }

    #[test]
    fn recv_bulk_with_handler_empty_body() {
        let frame = make_bulk_frame("actor1", "kind1", b"");
        let mut cursor = Cursor::new(frame);
        let mut out = Vec::new();
        let n = recv_bulk_with_handler_from(&mut cursor, "actor1", "kind1", &mut out).unwrap();
        assert_eq!(n, 0);
        assert!(out.is_empty());
    }

    #[test]
    fn recv_bulk_with_handler_actor_mismatch_returns_error() {
        let frame = make_bulk_frame("actor1", "kind1", b"hello");
        let mut cursor = Cursor::new(frame);
        let mut out = Vec::new();
        let err =
            recv_bulk_with_handler_from(&mut cursor, "actor2", "kind1", &mut out).unwrap_err();
        assert!(
            matches!(err, ProtocolError::BulkPacketUnexpected { .. }),
            "expected BulkPacketUnexpected, got {err:?}"
        );
    }

    #[test]
    fn recv_bulk_with_handler_kind_mismatch_returns_error() {
        let frame = make_bulk_frame("actor1", "kind1", b"hello");
        let mut cursor = Cursor::new(frame);
        let mut out = Vec::new();
        let err =
            recv_bulk_with_handler_from(&mut cursor, "actor1", "kind2", &mut out).unwrap_err();
        assert!(
            matches!(err, ProtocolError::BulkPacketUnexpected { .. }),
            "expected BulkPacketUnexpected, got {err:?}"
        );
    }

    #[test]
    fn recv_bulk_with_handler_json_frame_returns_unexpected() {
        // A JSON frame (not a bulk frame) → BulkPacketUnexpected.
        let payload = r#"{"type":"listTabs","to":"root"}"#;
        let frame = encode_frame(payload);
        let mut cursor = Cursor::new(frame.into_bytes());
        let mut out = Vec::new();
        let err =
            recv_bulk_with_handler_from(&mut cursor, "actor1", "kind1", &mut out).unwrap_err();
        assert!(
            matches!(err, ProtocolError::BulkPacketUnexpected { .. }),
            "expected BulkPacketUnexpected for JSON frame, got {err:?}"
        );
    }

    #[test]
    fn recv_bulk_with_handler_oversized_rejected() {
        // Header only — body > cap.
        let header = b"bulk actor1 kind1 8000000:";
        let mut cursor = Cursor::new(header.to_vec());
        let mut out = Vec::new();
        let err =
            recv_bulk_with_handler_from_with_cap(&mut cursor, "actor1", "kind1", &mut out, 1024)
                .unwrap_err();
        assert!(
            matches!(err, ProtocolError::BulkFrameTooLarge { .. }),
            "expected BulkFrameTooLarge, got {err:?}"
        );
    }

    // ── Theme A: bulk-frame drain tests ─────────────────────────────────────

    /// AC: `bulk_recv_drains_on_actor_mismatch` — after a mismatched bulk
    /// frame, the next `recv_from` returns the following frame intact.
    #[test]
    fn bulk_recv_drains_on_actor_mismatch() {
        // Build: bulk other-actor screenshot 30:<30 bytes> followed by a JSON frame.
        let body: Vec<u8> = b"X".repeat(30);
        let bulk_header = b"bulk other-actor screenshot 30:";
        let json_str = r#"{"from":"x","msg":"hello"}"#; // 25 bytes
        let json_frame = format!("{}:{}", json_str.len(), json_str);

        let mut stream = Vec::new();
        stream.extend_from_slice(bulk_header);
        stream.extend_from_slice(&body);
        stream.extend_from_slice(json_frame.as_bytes());

        let mut cursor = Cursor::new(stream);

        let mut out = Vec::new();
        let err =
            recv_bulk_with_handler_from(&mut cursor, "actor", "screenshot", &mut out).unwrap_err();
        assert!(
            matches!(err, ProtocolError::BulkPacketUnexpected { .. }),
            "expected BulkPacketUnexpected on actor mismatch, got {err:?}"
        );

        // Stream must be aligned: next recv_from should get the JSON frame.
        let val = recv_from(&mut cursor).expect("next frame must be readable after drain");
        assert_eq!(
            val.get("msg").and_then(serde_json::Value::as_str),
            Some("hello"),
            "next frame content mismatch"
        );
    }

    /// AC: `bulk_recv_drains_on_json_peek` — a JSON frame peeked by the bulk
    /// recv function is preserved for the next `recv_from`.
    #[test]
    fn bulk_recv_drains_on_json_peek() {
        let json_str = r#"{"from":"x","msg":"world"}"#; // 25 bytes
        let json_frame = format!("{}:{}", json_str.len(), json_str);

        let mut cursor = Cursor::new(json_frame.into_bytes());

        let mut out = Vec::new();
        let err =
            recv_bulk_with_handler_from(&mut cursor, "actor", "screenshot", &mut out).unwrap_err();
        assert!(
            matches!(err, ProtocolError::BulkPacketUnexpected { .. }),
            "expected BulkPacketUnexpected on JSON peek, got {err:?}"
        );

        // The JSON frame must still be intact (byte NOT consumed).
        let val = recv_from(&mut cursor).expect("JSON frame must be recoverable after peek");
        assert_eq!(
            val.get("msg").and_then(serde_json::Value::as_str),
            Some("world"),
            "JSON frame content mismatch"
        );
    }

    /// AC: `bulk_recv_caps_drain_length` — over-cap announced length is
    /// rejected before the discard loop (no body bytes read).
    #[test]
    fn bulk_recv_caps_drain_length() {
        // A very small cap, passed explicitly, so we can craft a frame that
        // exceeds it without shrinking the process-global cell.
        let cap = 100;

        // `drain_bulk_frame_with_cap` receives first_byte = b'b' (already consumed by
        // the caller).  The cursor starts with the rest of the header.
        // "ulk actor1 kind1 1000:" (22 bytes after 'b') + ':' terminator is
        // included in the string literal below; body bytes follow.
        //
        // Full header: "bulk actor1 kind1 1000:" (23 bytes total).
        // We pass 'b' as first_byte, so the cursor holds bytes 1..end.
        let rest_of_header = b"ulk actor1 kind1 1000:";
        // Provide only 10 body bytes (not the announced 1000).  If the cap
        // check fires first (correct), we get BulkFrameTooLarge before any
        // body read.  If it doesn't fire (wrong), we'd get RecvFailed on EOF.
        let short_body: Vec<u8> = b"X".repeat(10);

        let mut stream: Vec<u8> = Vec::new();
        stream.extend_from_slice(rest_of_header);
        stream.extend_from_slice(&short_body);
        let total_len = stream.len();

        let mut cursor = Cursor::new(stream);

        let res = drain_bulk_frame_with_cap(&mut cursor, b'b', cap);
        assert!(
            matches!(
                res,
                Err(ProtocolError::BulkFrameTooLarge {
                    announced: 1000,
                    max: 100
                })
            ),
            "expected BulkFrameTooLarge, got {res:?}"
        );

        // Cursor should be positioned right after the header ':' — body NOT read.
        // rest_of_header length = 22 bytes (includes the ':' at the end).
        #[allow(clippy::cast_possible_truncation)]
        let pos = cursor.position() as usize;
        assert_eq!(
            pos,
            rest_of_header.len(),
            "cursor should be positioned after header, not into body; \
             body bytes should still be unread (total={total_len}, pos={pos})"
        );
    }
}
