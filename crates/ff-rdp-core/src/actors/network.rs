use serde_json::Value;

use crate::actor::actor_request;
use crate::error::ProtocolError;
use crate::transport::RdpTransport;
use crate::types::ActorId;

/// An HTTP header name-value pair.
#[derive(Debug, Clone)]
pub struct Header {
    pub name: String,
    pub value: String,
}

/// Response content from a network event.
#[derive(Debug, Clone)]
pub struct ResponseContent {
    /// MIME type of the response.
    pub mime_type: String,
    /// Response body size in bytes.
    pub size: u64,
    /// Response body text (may be absent for binary content).
    pub text: Option<String>,
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
        let text = content
            .get("text")
            .and_then(Value::as_str)
            .map(String::from);

        Ok(ResponseContent {
            mime_type,
            size,
            text,
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
    use serde_json::json;

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
