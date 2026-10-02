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

/// A trimmed X.509 certificate summary extracted from a request's security
/// info.  Only the fields useful for a security audit are surfaced.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CertSummary {
    /// Certificate subject common name (e.g. the host the cert is issued for).
    pub subject: Option<String>,
    /// Issuer common name (the CA that signed the certificate).
    pub issuer: Option<String>,
    /// Not-before validity boundary, as Firefox reports it (a formatted string).
    pub valid_from: Option<String>,
    /// Not-after validity boundary, as Firefox reports it (a formatted string).
    pub valid_to: Option<String>,
    /// SHA-256 fingerprint of the certificate, when present.
    pub sha256_fingerprint: Option<String>,
}

/// Per-request TLS / certificate detail returned by
/// [`NetworkEventActor::get_security_info`].
///
/// This is a curated projection of Firefox's raw `securityInfo` payload (see
/// `kb/rdp/actors/network-event.md`): the fields a TLS/PWA audit cares about.
/// A plain-HTTP request has no security info at all, so the actor method
/// returns `None` in that case rather than an empty [`SecurityInfo`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct SecurityInfo {
    /// Connection security state as Firefox classifies it
    /// (`"secure"`, `"weak"`, `"insecure"`, `"broken"`).
    pub state: Option<String>,
    /// Negotiated TLS protocol version, e.g. `"TLSv1.3"`.  Values below
    /// `"TLSv1.2"` (i.e. TLS 1.0 / 1.1) are worth flagging in an audit.
    pub protocol_version: Option<String>,
    /// Negotiated cipher suite, e.g. `"TLS_AES_128_GCM_SHA256"`.
    pub cipher_suite: Option<String>,
    /// Whether the server sent an HSTS (`Strict-Transport-Security`) header.
    pub hsts: Option<bool>,
    /// Machine-readable reasons the connection was classified weak
    /// (e.g. `"cipher"`).  Empty for a clean secure connection.
    pub weakness_reasons: Vec<String>,
    /// Certificate summary, when Firefox attached one.
    pub cert: Option<CertSummary>,
}

impl SecurityInfo {
    /// Parse Firefox's raw `securityInfo` object into a [`SecurityInfo`].
    ///
    /// Tolerant of missing/unknown keys: each field is pulled out
    /// best-effort so a schema change in a future Firefox does not break the
    /// whole parse.  Returns a value even for a mostly-empty object; the
    /// caller decides how to treat an all-`None` result.
    pub fn from_value(v: &Value) -> Self {
        let state = v.get("state").and_then(Value::as_str).map(str::to_owned);
        let protocol_version = v
            .get("protocolVersion")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let cipher_suite = v
            .get("cipherSuite")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let hsts = v.get("hsts").and_then(Value::as_bool);
        let weakness_reasons = v
            .get("weaknessReasons")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|r| r.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        let cert = v.get("cert").map(parse_cert_summary);

        Self {
            state,
            protocol_version,
            cipher_suite,
            hsts,
            weakness_reasons,
            cert,
        }
    }
}

/// Extract a [`CertSummary`] from Firefox's `cert` object.
///
/// Firefox nests the common name under `subject.commonName` /
/// `issuer.commonName`, the validity window under `validity.start` /
/// `validity.end`, and the fingerprint under `fingerprint.sha256`.
fn parse_cert_summary(cert: &Value) -> CertSummary {
    let common_name = |key: &str| {
        cert.get(key)
            .and_then(|o| o.get("commonName"))
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    let validity = |key: &str| {
        cert.get("validity")
            .and_then(|v| v.get(key))
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    CertSummary {
        subject: common_name("subject"),
        issuer: common_name("issuer"),
        valid_from: validity("start"),
        valid_to: validity("end"),
        sha256_fingerprint: cert
            .get("fingerprint")
            .and_then(|f| f.get("sha256"))
            .and_then(Value::as_str)
            .map(str::to_owned),
    }
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

    /// Fetch per-request TLS / certificate detail for a network event.
    ///
    /// Returns `Some(SecurityInfo)` for HTTPS requests whose response Firefox
    /// observed, and `None` when the request carried no security info — either
    /// because it was plain HTTP (no TLS handshake) or because the watcher
    /// never saw the request (security info is populated only when the response
    /// is observed; see `network-event-actor.js:690-710`).
    pub fn get_security_info(
        transport: &mut RdpTransport,
        actor: &ActorId,
    ) -> Result<Option<SecurityInfo>, ProtocolError> {
        let reply = crate::specs::call::<crate::specs::network_event::GetSecurityInfo>(
            transport,
            actor,
            &crate::specs::NoArgs {},
        )?;
        Ok(reply
            .security_info
            .filter(|v| !v.is_null())
            .map(|v| SecurityInfo::from_value(&v)))
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
    fn security_info_from_value_extracts_fields() {
        let v = json!({
            "state": "secure",
            "protocolVersion": "TLSv1.3",
            "cipherSuite": "TLS_AES_128_GCM_SHA256",
            "hsts": true,
            "weaknessReasons": [],
            "cert": {
                "subject": {"commonName": "example.com"},
                "issuer": {"commonName": "DigiCert TLS RSA SHA256 2020 CA1"},
                "validity": {"start": "Mon, 01 Jan 2024", "end": "Wed, 01 Jan 2025"},
                "fingerprint": {"sha256": "AA:BB:CC"}
            }
        });
        let si = SecurityInfo::from_value(&v);
        assert_eq!(si.state.as_deref(), Some("secure"));
        assert_eq!(si.protocol_version.as_deref(), Some("TLSv1.3"));
        assert_eq!(si.cipher_suite.as_deref(), Some("TLS_AES_128_GCM_SHA256"));
        assert_eq!(si.hsts, Some(true));
        assert!(si.weakness_reasons.is_empty());
        let cert = si.cert.expect("cert present");
        assert_eq!(cert.subject.as_deref(), Some("example.com"));
        assert_eq!(
            cert.issuer.as_deref(),
            Some("DigiCert TLS RSA SHA256 2020 CA1")
        );
        assert_eq!(cert.valid_from.as_deref(), Some("Mon, 01 Jan 2024"));
        assert_eq!(cert.valid_to.as_deref(), Some("Wed, 01 Jan 2025"));
        assert_eq!(cert.sha256_fingerprint.as_deref(), Some("AA:BB:CC"));
    }

    #[test]
    fn security_info_from_value_tolerates_missing_keys() {
        let si = SecurityInfo::from_value(&json!({"state": "weak", "weaknessReasons": ["cipher"]}));
        assert_eq!(si.state.as_deref(), Some("weak"));
        assert_eq!(si.weakness_reasons, vec!["cipher".to_owned()]);
        assert!(si.protocol_version.is_none());
        assert!(si.cert.is_none());
    }

    #[test]
    fn get_security_info_https_returns_some() {
        let (mut transport, server) = make_transport_pair();
        let actor: ActorId = "server1.conn0.netEvent1".into();

        let srv = std::thread::spawn(move || {
            let mut reader = BufReader::new(server.try_clone().unwrap());
            let req = crate::transport::recv_from(&mut reader).unwrap();
            assert_eq!(req["type"], "getSecurityInfo");
            let resp = json!({
                "from": "server1.conn0.netEvent1",
                "securityInfo": {
                    "state": "secure",
                    "protocolVersion": "TLSv1.2",
                    "cipherSuite": "TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256",
                    "hsts": false,
                    "weaknessReasons": []
                }
            });
            send_frame(&server, &resp);
        });

        let si = NetworkEventActor::get_security_info(&mut transport, &actor)
            .unwrap()
            .expect("https request has security info");
        assert_eq!(si.protocol_version.as_deref(), Some("TLSv1.2"));
        assert!(si.cipher_suite.as_deref().is_some_and(|c| !c.is_empty()));
        srv.join().unwrap();
    }

    #[test]
    fn get_security_info_http_returns_none() {
        let (mut transport, server) = make_transport_pair();
        let actor: ActorId = "server1.conn0.netEvent2".into();

        let srv = std::thread::spawn(move || {
            let mut reader = BufReader::new(server.try_clone().unwrap());
            let _req = crate::transport::recv_from(&mut reader).unwrap();
            let resp = json!({
                "from": "server1.conn0.netEvent2",
                "securityInfo": null
            });
            send_frame(&server, &resp);
        });

        let si = NetworkEventActor::get_security_info(&mut transport, &actor).unwrap();
        assert!(si.is_none(), "plain-HTTP request has no security info");
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
