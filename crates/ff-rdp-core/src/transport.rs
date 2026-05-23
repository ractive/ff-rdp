use std::io::{BufReader, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use serde_json::Value;

use crate::error::ProtocolError;

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
        Self { reader, writer }
    }

    /// Decompose into the underlying reader/writer halves.
    ///
    /// Called by [`split`](Self::split) to hand the halves to `FramedReader`/`FramedWriter`.
    fn into_parts(self) -> (BufReader<TcpStream>, TcpStream) {
        (self.reader, self.writer)
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

/// Encode a JSON string as a Firefox RDP frame: `"{len}:{json}"`.
pub fn encode_frame(json: &str) -> String {
    format!("{}:{}", json.len(), json)
}

/// Read a single length-prefixed JSON packet from `reader`.
pub fn recv_from(reader: &mut impl Read) -> Result<Value, ProtocolError> {
    // Read one byte at a time until we hit ':'.
    let mut length_buf = Vec::with_capacity(10);
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

    if length_buf.is_empty() {
        return Err(ProtocolError::InvalidPacket(
            "empty length prefix".to_owned(),
        ));
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
        let mut transport = RdpTransport { reader, writer };

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
}
