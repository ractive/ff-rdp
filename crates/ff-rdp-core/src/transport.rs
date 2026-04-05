use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};

use crate::error::ProtocolError;

/// Low-level transport for the Firefox Remote Debugging Protocol.
///
/// Firefox uses a simple length-prefixed JSON framing over TCP:
/// - **Send**: `{byte_length}:{json_payload}`
/// - **Recv**: read ASCII digits until `:`, interpret as the byte count, then
///   read exactly that many bytes and parse as JSON.
pub struct RdpTransport {
    reader: OwnedReadHalf,
    writer: OwnedWriteHalf,
}

impl RdpTransport {
    /// Connect to a Firefox RDP server and consume the initial greeting packet.
    ///
    /// Firefox immediately sends a greeting after the TCP connection is
    /// established. We read and discard it so that the first call to
    /// [`recv`](Self::recv) returns an application-level message.
    pub async fn connect(host: &str, port: u16, timeout: Duration) -> Result<Self, ProtocolError> {
        let addr = format!("{host}:{port}");

        let stream = tokio::time::timeout(timeout, TcpStream::connect(&addr))
            .await
            .map_err(|_| ProtocolError::Timeout)?
            .map_err(ProtocolError::ConnectionFailed)?;

        let (reader, writer) = stream.into_split();
        let mut transport = Self { reader, writer };

        // Discard the Firefox greeting packet.
        transport.recv().await?;

        Ok(transport)
    }

    /// Send a JSON message using Firefox RDP framing: `{len}:{json}`.
    pub async fn send(&mut self, message: &Value) -> Result<(), ProtocolError> {
        let json = serde_json::to_string(message)
            .map_err(|e| ProtocolError::InvalidPacket(e.to_string()))?;

        let frame = encode_frame(&json);
        self.writer
            .write_all(frame.as_bytes())
            .await
            .map_err(ProtocolError::SendFailed)?;

        Ok(())
    }

    /// Receive a single length-prefixed JSON message.
    pub async fn recv(&mut self) -> Result<Value, ProtocolError> {
        recv_from(&mut self.reader).await
    }

    /// Send a request and immediately receive one response.
    pub async fn request(&mut self, message: &Value) -> Result<Value, ProtocolError> {
        self.send(message).await?;
        self.recv().await
    }
}

// ---------------------------------------------------------------------------
// Pure framing helpers — extracted so tests can exercise them without sockets.
// ---------------------------------------------------------------------------

/// Encode a JSON string as a Firefox RDP frame: `"{len}:{json}"`.
pub(crate) fn encode_frame(json: &str) -> String {
    format!("{}:{}", json.len(), json)
}

/// Read a single length-prefixed JSON packet from `reader`.
pub(crate) async fn recv_from(
    reader: &mut (impl AsyncReadExt + Unpin),
) -> Result<Value, ProtocolError> {
    // Read one byte at a time until we hit ':'.
    let mut length_buf = Vec::with_capacity(10);
    loop {
        let mut byte = [0u8; 1];
        reader
            .read_exact(&mut byte)
            .await
            .map_err(ProtocolError::RecvFailed)?;

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
        if length_buf.len() > 20 {
            return Err(ProtocolError::InvalidPacket(
                "length prefix exceeds 20 digits".to_owned(),
            ));
        }
    }

    if length_buf.is_empty() {
        return Err(ProtocolError::InvalidPacket(
            "empty length prefix".to_owned(),
        ));
    }

    let length_str =
        std::str::from_utf8(&length_buf).expect("already verified all bytes are ASCII digits");

    let length: usize = length_str
        .parse()
        .map_err(|e| ProtocolError::InvalidPacket(format!("length parse error: {e}")))?;

    let mut body = vec![0u8; length];
    reader
        .read_exact(&mut body)
        .await
        .map_err(ProtocolError::RecvFailed)?;

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

    #[tokio::test]
    async fn recv_parses_valid_frame() {
        let payload = r#"{"type":"listTabs","to":"root"}"#;
        let frame = encode_frame(payload);
        let mut cursor = Cursor::new(frame.into_bytes());

        let value = recv_from(&mut cursor).await.unwrap();
        assert_eq!(value["type"], "listTabs");
        assert_eq!(value["to"], "root");
    }

    #[tokio::test]
    async fn recv_handles_multi_digit_length() {
        let long_value: String = "x".repeat(200);
        let payload = serde_json::to_string(&serde_json::json!({"v": long_value})).unwrap();
        assert!(payload.len() >= 100, "payload must have a 3-digit length");

        let frame = encode_frame(&payload);
        let mut cursor = Cursor::new(frame.into_bytes());

        let value = recv_from(&mut cursor).await.unwrap();
        assert_eq!(value["v"].as_str().unwrap(), long_value);
    }

    #[tokio::test]
    async fn recv_errors_on_non_digit_in_length_prefix() {
        let bad = b"x:{}";
        let mut cursor = Cursor::new(bad.as_ref());

        let err = recv_from(&mut cursor).await.unwrap_err();
        assert!(
            matches!(err, ProtocolError::InvalidPacket(_)),
            "expected InvalidPacket, got {err:?}"
        );
    }

    #[tokio::test]
    async fn recv_errors_on_empty_length_prefix() {
        let bad = b":{}";
        let mut cursor = Cursor::new(bad.as_ref());

        let err = recv_from(&mut cursor).await.unwrap_err();
        assert!(
            matches!(err, ProtocolError::InvalidPacket(_)),
            "expected InvalidPacket, got {err:?}"
        );
    }

    #[tokio::test]
    async fn recv_errors_on_invalid_json_body() {
        let bad_body = b"not-json";
        let frame = format!("{}:{}", bad_body.len(), String::from_utf8_lossy(bad_body));
        let mut cursor = Cursor::new(frame.into_bytes());

        let err = recv_from(&mut cursor).await.unwrap_err();
        assert!(
            matches!(err, ProtocolError::InvalidPacket(_)),
            "expected InvalidPacket, got {err:?}"
        );
    }

    #[tokio::test]
    async fn recv_errors_on_length_prefix_too_long() {
        // 21 consecutive digit bytes with no colon triggers the guard.
        let frame = "1".repeat(21);
        let mut cursor = Cursor::new(frame.into_bytes());

        let err = recv_from(&mut cursor).await.unwrap_err();
        assert!(
            matches!(err, ProtocolError::InvalidPacket(_)),
            "expected InvalidPacket, got {err:?}"
        );
    }

    // -----------------------------------------------------------------------
    // send via RdpTransport — minimal loopback test
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn send_produces_correct_frame_over_socket() {
        use tokio::io::AsyncReadExt;
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        // Connect client before accepting so the handshake completes.
        let client_stream = TcpStream::connect(addr).await.unwrap();
        let (server_stream, _) = listener.accept().await.unwrap();

        let (reader, writer) = client_stream.into_split();
        let mut transport = RdpTransport { reader, writer };

        let msg = serde_json::json!({"type": "listTabs", "to": "root"});
        transport.send(&msg).await.unwrap();

        // Drop the transport's writer so the server sees EOF.
        drop(transport);

        let (mut srv_reader, _srv_writer) = server_stream.into_split();
        let mut buf = Vec::new();
        srv_reader.read_to_end(&mut buf).await.unwrap();

        let raw = String::from_utf8(buf).unwrap();
        let expected_json = serde_json::to_string(&msg).unwrap();
        assert_eq!(raw, encode_frame(&expected_json));
    }
}
