use std::io::{BufReader, Read, Write};
use std::net::TcpStream;
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

/// Maximum string length (in bytes) allowed through the redactor for ad-hoc
/// string values that aren't explicitly listed in `SENSITIVE_KEYS`.
///
/// Any string exceeding this threshold inside a traced packet is replaced with
/// `<redacted len=N>`.
const MAX_INLINE_STR: usize = 32;

/// Redact a JSON value and return a redacted clone for safe trace output.
///
/// - All values of keys matching [`SENSITIVE_KEYS`] are replaced.
/// - All values of keys matching [`SOURCE_KEYS`] are replaced.
/// - String values exceeding [`MAX_INLINE_STR`] anywhere in the tree are
///   replaced.
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
            if s.len() > MAX_INLINE_STR {
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

/// Maximum frame payload size accepted from a Firefox RDP peer.
///
/// 64 MiB comfortably exceeds the largest legitimate frame observed in the
/// wild (full-page screenshot data URLs).  Frames declaring a larger length
/// are rejected before any allocation is attempted — this prevents a
/// malformed or malicious peer from causing an immediate OOM abort.
pub const MAX_FRAME_BYTES: usize = 64 * 1024 * 1024;

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

        Ok(value)
    }

    /// Send a request and immediately receive one response.
    pub fn request(&mut self, message: &Value) -> Result<Value, ProtocolError> {
        self.send(message)?;
        self.recv()
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
// Pure framing helpers — extracted so tests can exercise them without sockets.
// ---------------------------------------------------------------------------

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
/// loop continues. Packets from other actors are ignored.
///
/// On `error`-bearing replies, the helper converts the packet into a
/// [`ProtocolError::ActorError`] using [`ActorErrorKind::from_code`].
pub fn recv_reply_from(transport: &mut RdpTransport, actor: &str) -> Result<Value, ProtocolError> {
    loop {
        let msg = transport.recv()?;
        let from = msg.get("from").and_then(Value::as_str).unwrap_or_default();
        if from != actor {
            // Foreign packet — not ours to interpret; drop on the floor for
            // now (callers that care set their own event_sink + use
            // recv_event_from with a predicate).
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
/// from a known actor. Packets from other actors are skipped silently; events
/// from `actor` that do not match the predicate are also skipped (they aren't
/// forwarded to the event sink because the caller has explicit semantics for
/// what counts as "their" event).
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
        }
    }
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
pub fn recv_from(reader: &mut impl Read) -> Result<Value, ProtocolError> {
    // Read the first byte to distinguish JSON vs bulk frames.
    let mut first = [0u8; 1];
    reader.read_exact(&mut first).map_err(map_recv_io_error)?;

    if first[0] == b'b' {
        return recv_bulk_frame(reader);
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
    // than MAX_FRAME_BYTES is either corrupted or malicious.
    if length > MAX_FRAME_BYTES {
        return Err(ProtocolError::FrameTooLarge {
            declared: length,
            max: MAX_FRAME_BYTES,
        });
    }

    let mut body = vec![0u8; length];
    reader.read_exact(&mut body).map_err(map_recv_io_error)?;

    let value = serde_json::from_slice(&body)
        .map_err(|e| ProtocolError::InvalidPacket(format!("JSON parse error: {e}")))?;

    Ok(value)
}

/// Parse and discard a Firefox bulk frame.
///
/// Called when `recv_from` sees a leading `b`.  The bulk frame header format is:
/// `bulk <actor> <kind> <length>:` followed by exactly `length` binary bytes.
/// The `b` has already been consumed; this function reads the rest of the header
/// and skips the body.
///
/// Returns [`ProtocolError::BulkPacketUnsupported`] on success (body skipped)
/// or a parse/IO error if the stream is malformed.
fn recv_bulk_frame(reader: &mut impl Read) -> Result<Value, ProtocolError> {
    // We already consumed the leading 'b'.  Read up to ':' to get the rest of
    // the header: "ulk <actor> <kind> <length>".
    let mut header_buf: Vec<u8> = b"b".to_vec();
    loop {
        let mut byte = [0u8; 1];
        reader.read_exact(&mut byte).map_err(map_recv_io_error)?;

        if byte[0] == b':' {
            break;
        }
        header_buf.push(byte[0]);

        // Sanity limit: headers shouldn't be multi-KB.
        if header_buf.len() > 4096 {
            return Err(ProtocolError::InvalidPacket(
                "bulk frame header exceeds 4096 bytes".to_owned(),
            ));
        }
    }

    let header = std::str::from_utf8(&header_buf)
        .map_err(|_| ProtocolError::InvalidPacket("non-UTF8 in bulk frame header".to_owned()))?;

    // Expected format: "bulk <actor> <kind> <length>"
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

    // Consume (discard) the body bytes in chunks to avoid large allocations.
    let mut remaining = length;
    let mut discard = [0u8; 8192];
    while remaining > 0 {
        let chunk_len = discard.len().min(
            // Safe: remaining fits in usize because we take at most discard.len().
            usize::try_from(remaining).unwrap_or(discard.len()),
        );
        reader
            .read_exact(&mut discard[..chunk_len])
            .map_err(map_recv_io_error)?;
        remaining -= chunk_len as u64;
    }

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
        // Declare a 100 MB frame (> MAX_FRAME_BYTES = 64 MiB).  No allocation
        // should happen — the error must be returned before reading the body.
        // We only send the length prefix followed by a colon; the cursor has
        // no body bytes, so if recv_from tried to allocate and read we would
        // get a RecvFailed instead of FrameTooLarge.
        let declared = 100_000_000usize;
        let prefix = format!("{declared}:");
        let mut cursor = Cursor::new(prefix.into_bytes());

        let err = recv_from(&mut cursor).unwrap_err();
        assert!(
            matches!(
                err,
                ProtocolError::FrameTooLarge {
                    declared: 100_000_000,
                    max: _
                }
            ),
            "expected FrameTooLarge, got {err:?}"
        );
    }

    // -----------------------------------------------------------------------
    // redact — pure unit tests
    // -----------------------------------------------------------------------

    #[test]
    fn redact_sensitive_key_replaces_value() {
        let v = serde_json::json!({"cookie": "session=abc123"});
        let r = redact(&v);
        let s = r["cookie"].as_str().unwrap();
        assert!(s.starts_with("<redacted"), "expected redaction, got: {s}");
    }

    #[test]
    fn redact_source_key_replaces_value() {
        let v = serde_json::json!({"text": "console.log('hello')"});
        let r = redact(&v);
        let s = r["text"].as_str().unwrap();
        assert!(s.starts_with("<redacted"), "expected redaction, got: {s}");
    }

    #[test]
    fn redact_long_string_replaces_value() {
        let long = "x".repeat(MAX_INLINE_STR + 1);
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

    /// `recv_reply_from` must skip foreign packets without forwarding them
    /// (they aren't ours; the caller's event sink only sees packets from the
    /// targeted actor).
    #[test]
    fn recv_reply_from_skips_foreign_packets() {
        let (mut transport, server) = make_transport_pair();
        let (tx, rx) = std::sync::mpsc::channel::<Value>();
        transport.set_event_sink(Some(tx));

        let server_thread = std::thread::spawn(move || {
            write_frame(
                &server,
                &serde_json::json!({"from": "otherActor", "type": "tabListChanged"}),
            );
            write_frame(&server, &serde_json::json!({"from": "actorA", "ok": true}));
        });

        let reply = recv_reply_from(&mut transport, "actorA").unwrap();
        assert_eq!(reply["ok"], true);

        assert!(
            rx.try_recv().is_err(),
            "foreign packets must NOT be forwarded to the event sink"
        );
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
}
