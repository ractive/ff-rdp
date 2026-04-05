use std::io::{BufReader, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use serde_json::Value;

use crate::error::ProtocolError;

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

        let addr_str = format!("{host}:{port}");
        let addr = addr_str
            .to_socket_addrs()
            .map_err(ProtocolError::ConnectionFailed)?
            .next()
            .ok_or_else(|| {
                ProtocolError::ConnectionFailed(std::io::Error::new(
                    std::io::ErrorKind::AddrNotAvailable,
                    format!("could not resolve {addr_str}"),
                ))
            })?;

        let stream = TcpStream::connect_timeout(&addr, timeout).map_err(|e| {
            if e.kind() == std::io::ErrorKind::TimedOut {
                ProtocolError::Timeout
            } else {
                ProtocolError::ConnectionFailed(e)
            }
        })?;

        stream
            .set_read_timeout(Some(timeout))
            .map_err(ProtocolError::ConnectionFailed)?;
        stream
            .set_write_timeout(Some(timeout))
            .map_err(ProtocolError::ConnectionFailed)?;

        let writer = stream
            .try_clone()
            .map_err(ProtocolError::ConnectionFailed)?;
        let reader = BufReader::new(stream);

        Ok(Self { reader, writer })
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
    pub fn from_parts(reader: BufReader<TcpStream>, writer: TcpStream) -> Self {
        Self { reader, writer }
    }

    /// Send a JSON message using Firefox RDP framing: `{len}:{json}`.
    pub fn send(&mut self, message: &Value) -> Result<(), ProtocolError> {
        let json = serde_json::to_string(message)
            .map_err(|e| ProtocolError::InvalidPacket(e.to_string()))?;

        let frame = encode_frame(&json);
        self.writer.write_all(frame.as_bytes()).map_err(|e| {
            if e.kind() == std::io::ErrorKind::TimedOut
                || e.kind() == std::io::ErrorKind::WouldBlock
            {
                ProtocolError::Timeout
            } else {
                ProtocolError::SendFailed(e)
            }
        })?;

        Ok(())
    }

    /// Receive a single length-prefixed JSON message.
    pub fn recv(&mut self) -> Result<Value, ProtocolError> {
        recv_from(&mut self.reader)
    }

    /// Send a request and immediately receive one response.
    pub fn request(&mut self, message: &Value) -> Result<Value, ProtocolError> {
        self.send(message)?;
        self.recv()
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
        reader.read_exact(&mut byte).map_err(|e| {
            if e.kind() == std::io::ErrorKind::TimedOut
                || e.kind() == std::io::ErrorKind::WouldBlock
            {
                ProtocolError::Timeout
            } else {
                ProtocolError::RecvFailed(e)
            }
        })?;

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

    let mut body = vec![0u8; length];
    reader.read_exact(&mut body).map_err(|e| {
        if e.kind() == std::io::ErrorKind::TimedOut || e.kind() == std::io::ErrorKind::WouldBlock {
            ProtocolError::Timeout
        } else {
            ProtocolError::RecvFailed(e)
        }
    })?;

    let value = serde_json::from_slice(&body)
        .map_err(|e| ProtocolError::InvalidPacket(format!("JSON parse error: {e}")))?;

    Ok(value)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

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
