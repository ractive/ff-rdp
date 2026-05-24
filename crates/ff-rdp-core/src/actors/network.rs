use serde_json::Value;

use crate::actor::actor_request;
use crate::actors::string::LongStringActor;
use crate::error::ProtocolError;
use crate::transport::{RdpTransport, max_frame_bytes};
use crate::types::ActorId;

/// An HTTP header name-value pair.
#[derive(Debug, Clone)]
pub struct Header {
    pub name: String,
    pub value: String,
}

/// Response content from a network event.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ResponseContent {
    /// MIME type of the response.
    pub mime_type: String,
    /// Response body size in bytes.
    pub size: u64,
    /// Response body text (may be absent for binary content).
    pub text: Option<String>,
    /// True when the body was truncated at the configured frame-size cap
    /// (see [`max_frame_bytes`]) due to size limits.
    pub truncated: bool,
}

/// Timing information for a network event.
#[derive(Debug, Clone)]
pub struct EventTimings {
    pub blocked: f64,
    pub dns: f64,
    pub connect: f64,
    pub ssl: f64,
    pub send: f64,
    pub wait: f64,
    pub receive: f64,
    pub total: f64,
}

/// Operations on a NetworkEvent actor for fetching request/response details.
///
/// A `NetworkEventActor` ID is obtained from `resources-available-array` events
/// after calling `WatcherActor::watch_resources` with `"network-event"`.
pub struct NetworkEventActor;

impl NetworkEventActor {
    /// Fetch the request headers for a network event.
    pub fn get_request_headers(
        transport: &mut RdpTransport,
        actor: &ActorId,
    ) -> Result<Vec<Header>, ProtocolError> {
        let response = actor_request(transport, actor.as_ref(), "getRequestHeaders", None)?;
        Ok(parse_headers(&response))
    }

    /// Fetch the response headers for a network event.
    pub fn get_response_headers(
        transport: &mut RdpTransport,
        actor: &ActorId,
    ) -> Result<Vec<Header>, ProtocolError> {
        let response = actor_request(transport, actor.as_ref(), "getResponseHeaders", None)?;
        Ok(parse_headers(&response))
    }

    /// Fetch the response body content for a network event.
    ///
    /// Firefox returns the body as either a plain string or a `longString`
    /// grip (`{type:"longString", actor, initial, length}`) for large bodies.
    /// When a `longString` grip is detected, the full content is fetched via
    /// [`LongStringActor::full_string`] in chunks, capped at
    /// the current frame-size cap (see [`max_frame_bytes`]).  Bodies
    /// exceeding the cap are truncated and `truncated` is set to `true` in
    /// the result.
    pub fn get_response_content(
        transport: &mut RdpTransport,
        actor: &ActorId,
    ) -> Result<ResponseContent, ProtocolError> {
        let response = actor_request(transport, actor.as_ref(), "getResponseContent", None)?;

        let content = response.get("content").unwrap_or(&Value::Null);
        let mime_type = content
            .get("mimeType")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let size = content.get("size").and_then(Value::as_u64).unwrap_or(0);

        // `text` may be a plain string or a longString grip object.
        let text_value = content.get("text").unwrap_or(&Value::Null);
        let (text, truncated) = if let Some(s) = text_value.as_str() {
            // Inline string — common for small responses.
            (Some(s.to_owned()), false)
        } else if let (Some("longString"), Some(long_actor), Some(declared_len)) = (
            text_value.get("type").and_then(Value::as_str),
            text_value.get("actor").and_then(Value::as_str),
            text_value.get("length").and_then(Value::as_u64),
        ) {
            // longString grip — fetch the full body, capped at the
            // configured maximum frame size.
            let cap = u64::try_from(max_frame_bytes()).unwrap_or(u64::MAX);
            let fetch_len = declared_len.min(cap);
            let body = LongStringActor::full_string(transport, long_actor, fetch_len)?;
            let truncated = declared_len > cap;
            (Some(body), truncated)
        } else {
            // Null, binary, or unknown grip shape — body not available as text.
            (None, false)
        };

        Ok(ResponseContent {
            mime_type,
            size,
            text,
            truncated,
        })
    }

    /// Fetch timing information for a network event.
    pub fn get_event_timings(
        transport: &mut RdpTransport,
        actor: &ActorId,
    ) -> Result<EventTimings, ProtocolError> {
        let response = actor_request(transport, actor.as_ref(), "getEventTimings", None)?;

        let timings = response.get("timings").unwrap_or(&Value::Null);
        let total = response
            .get("totalTime")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);

        Ok(EventTimings {
            blocked: timings
                .get("blocked")
                .and_then(Value::as_f64)
                .unwrap_or(0.0),
            dns: timings.get("dns").and_then(Value::as_f64).unwrap_or(0.0),
            connect: timings
                .get("connect")
                .and_then(Value::as_f64)
                .unwrap_or(0.0),
            ssl: timings.get("ssl").and_then(Value::as_f64).unwrap_or(0.0),
            send: timings.get("send").and_then(Value::as_f64).unwrap_or(0.0),
            wait: timings.get("wait").and_then(Value::as_f64).unwrap_or(0.0),
            receive: timings
                .get("receive")
                .and_then(Value::as_f64)
                .unwrap_or(0.0),
            total,
        })
    }
}

fn parse_headers(response: &Value) -> Vec<Header> {
    response
        .get("headers")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|h| {
                    let name = h.get("name").and_then(Value::as_str)?;
                    let value = h.get("value").and_then(Value::as_str)?;
                    Some(Header {
                        name: name.to_owned(),
                        value: value.to_owned(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::{RdpTransport, encode_frame};
    use serde_json::json;
    use std::io::{BufReader, Write};
    use std::net::{TcpListener, TcpStream};

    fn make_transport_pair() -> (RdpTransport, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let client = TcpStream::connect(addr).unwrap();
        let (server_stream, _) = listener.accept().unwrap();
        let writer = client.try_clone().unwrap();
        let reader = BufReader::new(client);
        (RdpTransport::from_parts(reader, writer), server_stream)
    }

    fn send_frame(stream: &TcpStream, msg: &serde_json::Value) {
        let json = serde_json::to_string(msg).unwrap();
        stream
            .try_clone()
            .unwrap()
            .write_all(encode_frame(&json).as_bytes())
            .unwrap();
    }

    #[test]
    fn get_response_content_unwraps_long_string_grip() {
        let (mut transport, server) = make_transport_pair();
        let actor: ActorId = "server1.conn0.netEvent1".into();
        let long_string_actor = "server1.conn0.longstr42";

        let srv = std::thread::spawn(move || {
            let mut reader = BufReader::new(server.try_clone().unwrap());

            // Consume getResponseContent request — reply has no `type` field.
            let _req = crate::transport::recv_from(&mut reader).unwrap();
            let content_resp = json!({
                "from": "server1.conn0.netEvent1",
                "content": {
                    "mimeType": "application/json",
                    "size": 20000,
                    "text": {
                        "type": "longString",
                        "actor": long_string_actor,
                        "initial": "first 1000 chars",
                        "length": 20000_u64
                    }
                }
            });
            send_frame(&server, &content_resp);

            // Consume the substring request — reply has no `type` field.
            let _sub_req = crate::transport::recv_from(&mut reader).unwrap();
            let sub_resp = json!({
                "from": long_string_actor,
                "substring": "full body content here"
            });
            send_frame(&server, &sub_resp);
        });

        let content = NetworkEventActor::get_response_content(&mut transport, &actor).unwrap();
        assert_eq!(content.mime_type, "application/json");
        assert_eq!(content.size, 20000);
        assert_eq!(content.text.as_deref(), Some("full body content here"));
        assert!(!content.truncated);

        srv.join().unwrap();
    }

    #[test]
    fn get_response_content_inline_string() {
        let (mut transport, server) = make_transport_pair();
        let actor: ActorId = "server1.conn0.netEvent2".into();

        let srv = std::thread::spawn(move || {
            let mut reader = BufReader::new(server.try_clone().unwrap());
            let _req = crate::transport::recv_from(&mut reader).unwrap();
            let resp = json!({
                "from": "server1.conn0.netEvent2",
                "content": {
                    "mimeType": "text/plain",
                    "size": 5,
                    "text": "hello"
                }
            });
            send_frame(&server, &resp);
        });

        let content = NetworkEventActor::get_response_content(&mut transport, &actor).unwrap();
        assert_eq!(content.text.as_deref(), Some("hello"));
        assert!(!content.truncated);

        srv.join().unwrap();
    }

    #[test]
    fn parse_headers_from_response() {
        let response = json!({
            "from": "server1.conn0.netEvent6",
            "headers": [
                {"name": "Host", "value": "example.com"},
                {"name": "Accept", "value": "text/html"}
            ],
            "headersSize": 0
        });

        let headers = parse_headers(&response);
        assert_eq!(headers.len(), 2);
        assert_eq!(headers[0].name, "Host");
        assert_eq!(headers[0].value, "example.com");
        assert_eq!(headers[1].name, "Accept");
        assert_eq!(headers[1].value, "text/html");
    }

    #[test]
    fn parse_headers_empty() {
        let response = json!({"from": "actor1", "headers": []});
        assert!(parse_headers(&response).is_empty());
    }

    #[test]
    fn parse_headers_missing_field() {
        let response = json!({"from": "actor1"});
        assert!(parse_headers(&response).is_empty());
    }
}
