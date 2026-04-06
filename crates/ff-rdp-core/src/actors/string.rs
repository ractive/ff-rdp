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

    /// Convenience: fetch the full content of a long string actor.
    pub fn full_string(
        transport: &mut RdpTransport,
        actor: &str,
        length: u64,
    ) -> Result<String, ProtocolError> {
        Self::substring(transport, actor, 0, length)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::{RdpTransport, encode_frame};
    use std::io::{BufReader, Write};
    use std::net::TcpListener;
    use std::time::Duration;

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
