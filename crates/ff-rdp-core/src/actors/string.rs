use crate::actor::actor_request;
use crate::error::ProtocolError;
use crate::transport::RdpTransport;

/// Actor for fetching full content from Firefox long strings.
///
/// When a string result exceeds Firefox's inline limit (~1000 chars), it is
/// returned as a `longString` grip with an actor ID, initial prefix, and total
/// length. Use this actor to fetch the complete string via `substring`.
pub struct LongStringActor;

impl LongStringActor {
    /// Fetch a substring from a long string actor.
    ///
    /// Firefox's `StringActor` responds to `substring` requests with the
    /// content between `start` and `end` byte offsets.
    pub fn substring(
        transport: &mut RdpTransport,
        actor: &str,
        start: u64,
        end: u64,
    ) -> Result<String, ProtocolError> {
        let params = serde_json::json!({ "start": start, "end": end });
        let resp = actor_request(transport, actor, "substring", Some(&params))?;
        resp.get("substring")
            .and_then(serde_json::Value::as_str)
            .map(String::from)
            .ok_or_else(|| {
                ProtocolError::InvalidPacket("substring response missing 'substring' field".into())
            })
    }

    /// Maximum byte length accepted by [`Self::full_string`].
    ///
    /// Strings larger than this are rejected before any allocation is made to
    /// prevent memory exhaustion from a malicious or buggy Firefox response.
    /// 16 MiB is well above any page-text or computed-style payload we expect
    /// in practice.
    pub const MAX_FETCH: usize = 16 * 1024 * 1024;

    /// Convenience: fetch the full content of a long string actor.
    ///
    /// Firefox's substring protocol may enforce a maximum response size per
    /// call. Fetches in 65 536-character chunks and concatenates them so that
    /// arbitrarily large strings are handled correctly.
    ///
    /// Returns [`ProtocolError::InvalidPacket`] without any allocation when
    /// `length` exceeds [`Self::MAX_FETCH`] or cannot be converted to `usize`.
    pub fn full_string(
        transport: &mut RdpTransport,
        actor: &str,
        length: u64,
    ) -> Result<String, ProtocolError> {
        const CHUNK_SIZE: u64 = 65536;

        // Guard against oversized or platform-unrepresentable lengths before
        // making any allocation.
        let length_usize = usize::try_from(length).unwrap_or(usize::MAX);
        if length_usize > Self::MAX_FETCH {
            return Err(ProtocolError::InvalidPacket(format!(
                "longstring too large: {length} bytes (max {})",
                Self::MAX_FETCH
            )));
        }

        if length <= CHUNK_SIZE {
            return Self::substring(transport, actor, 0, length);
        }
        let mut result = String::with_capacity(length_usize);
        let mut offset = 0;
        while offset < length {
            let end = (offset + CHUNK_SIZE).min(length);
            let chunk = Self::substring(transport, actor, offset, end)?;
            result.push_str(&chunk);
            offset = end;
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::{RdpTransport, encode_frame};
    use std::io::{BufReader, Write};
    use std::net::TcpListener;
    use std::time::Duration;

    // -----------------------------------------------------------------------
    // full_string — MAX_FETCH cap (Theme E, iter-61w)
    // -----------------------------------------------------------------------

    #[test]
    fn full_string_rejects_length_above_max_fetch_with_zero_allocation() {
        // u64::MAX is far above MAX_FETCH; the error must be returned before
        // any allocation or network I/O — demonstrated by using a port that
        // nothing is listening on (no connect attempt should be made either,
        // since we short-circuit in full_string before calling substring).
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        // Accept in a thread so the client connect doesn't block — though we
        // expect full_string to error before it even calls substring/connect.
        let _handle = std::thread::spawn(move || {
            let _ = listener.accept(); // may or may not be called
        });

        let mut transport =
            RdpTransport::connect_raw("127.0.0.1", port, Duration::from_secs(5)).unwrap();
        // Use a length larger than MAX_FETCH but representable as u64.
        let oversized: u64 = (LongStringActor::MAX_FETCH as u64) + 1;
        let err = LongStringActor::full_string(&mut transport, "longstr1", oversized).unwrap_err();
        assert!(
            matches!(err, crate::error::ProtocolError::InvalidPacket(ref msg) if msg.contains("longstring too large")),
            "expected InvalidPacket with 'longstring too large', got: {err:?}"
        );
    }

    #[test]
    fn full_string_rejects_u64_max_with_zero_allocation() {
        // u64::MAX cannot even be represented as usize on 32-bit platforms
        // (usize::try_from will fail).  The check must fire before any alloc.
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let _handle = std::thread::spawn(move || {
            let _ = listener.accept();
        });

        let mut transport =
            RdpTransport::connect_raw("127.0.0.1", port, Duration::from_secs(5)).unwrap();
        let err = LongStringActor::full_string(&mut transport, "longstr1", u64::MAX).unwrap_err();
        assert!(
            matches!(err, crate::error::ProtocolError::InvalidPacket(ref msg) if msg.contains("longstring too large")),
            "expected InvalidPacket with 'longstring too large', got: {err:?}"
        );
    }

    #[test]
    fn substring_returns_content() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let handle = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut writer = stream.try_clone().unwrap();
            let mut reader = BufReader::new(stream);

            // Send greeting
            let greeting =
                serde_json::json!({"from":"root","applicationType":"browser","traits":{}});
            writer
                .write_all(encode_frame(&serde_json::to_string(&greeting).unwrap()).as_bytes())
                .unwrap();

            // Read the substring request
            let _req = crate::transport::recv_from(&mut reader).unwrap();

            // Send response
            let resp = serde_json::json!({"from":"longstr1","substring":"the full long string content here"});
            writer
                .write_all(encode_frame(&serde_json::to_string(&resp).unwrap()).as_bytes())
                .unwrap();
        });

        let mut transport =
            RdpTransport::connect("127.0.0.1", port, Duration::from_secs(5)).unwrap();
        let result = LongStringActor::substring(&mut transport, "longstr1", 0, 50000).unwrap();
        assert_eq!(result, "the full long string content here");

        handle.join().unwrap();
    }
}
