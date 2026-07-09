//! Spec for the NetworkEvent actor (per-request detail actor).
//!
//! Mirrors <https://searchfox.org/mozilla-central/source/devtools/shared/specs/network-event.js>
//!
//! A `NetworkEventActor` ID is obtained from `resources-available-array` events
//! after calling `watchResources` with `"network-event"`.

use serde::Deserialize;
use serde_json::Value;

use super::{Method, NoArgs, sealed, types::LongString};

// Re-export the typed structs the actor module uses, so callers use the spec as the surface.
pub use crate::actors::network::{EventTimings, Header, ResponseContent};
// Re-export LongString so callers can resolve actor references without importing from types.
pub use super::types::LongString as HeaderValue;

// ---------------------------------------------------------------------------
// Request args
// ---------------------------------------------------------------------------

pub mod request {
    use super::NoArgs;

    /// Args for `getRequestHeaders` — no parameters.
    pub type GetRequestHeaders = NoArgs;

    /// Args for `getResponseHeaders` — no parameters.
    pub type GetResponseHeaders = NoArgs;

    /// Args for `getResponseContent` — no parameters.
    pub type GetResponseContent = NoArgs;

    /// Args for `getSecurityInfo` — no parameters.
    pub type GetSecurityInfo = NoArgs;
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

pub mod response {
    use super::{Deserialize, Value};

    /// A single header entry from Firefox.
    ///
    /// The `value` field may be an inline string or a `longString` actor reference
    /// (for large headers such as `Set-Cookie` or `Content-Security-Policy`).
    /// Use [`super::LongString::fetch_full`] to obtain the full value when needed.
    #[derive(Debug, Clone, Default, Deserialize)]
    pub struct HeaderEntry {
        pub name: String,
        pub value: super::LongString,
    }

    /// Reply for `getRequestHeaders`.
    #[derive(Debug, Clone, Default, Deserialize)]
    pub struct GetRequestHeaders {
        #[serde(default)]
        pub headers: Vec<HeaderEntry>,
        #[serde(rename = "headersSize", default)]
        pub headers_size: u64,
    }

    /// Reply for `getResponseHeaders`.
    #[derive(Debug, Clone, Default, Deserialize)]
    pub struct GetResponseHeaders {
        #[serde(default)]
        pub headers: Vec<HeaderEntry>,
        #[serde(rename = "headersSize", default)]
        pub headers_size: u64,
    }

    /// Reply for `getResponseContent`.
    #[derive(Debug, Clone, Default, Deserialize)]
    pub struct GetResponseContent {
        pub content: Option<Value>,
    }

    /// Reply for `getSecurityInfo`.
    ///
    /// The `securityInfo` field carries the whole security payload Firefox
    /// caches for a request (populated when the response was observed — see
    /// `network-event-actor.js:340-360` / `690-710`). It is `null` for
    /// plain-HTTP requests (no TLS handshake) and for requests the watcher
    /// never observed.  The full shape is documented in
    /// `kb/rdp/actors/network-event.md`; we deserialise the raw object and let
    /// [`crate::actors::network::SecurityInfo::from_value`] pull out the fields
    /// we surface so unknown keys are tolerated.
    #[derive(Debug, Clone, Default, Deserialize)]
    pub struct GetSecurityInfo {
        #[serde(rename = "securityInfo", default)]
        pub security_info: Option<Value>,
    }
}

// ---------------------------------------------------------------------------
// Method markers
// ---------------------------------------------------------------------------

/// `getRequestHeaders` method marker.
pub struct GetRequestHeaders;
impl sealed::Sealed for GetRequestHeaders {}
impl Method for GetRequestHeaders {
    const NAME: &'static str = "getRequestHeaders";
    type Args = NoArgs;
    type Reply = response::GetRequestHeaders;
}

/// `getResponseHeaders` method marker.
pub struct GetResponseHeaders;
impl sealed::Sealed for GetResponseHeaders {}
impl Method for GetResponseHeaders {
    const NAME: &'static str = "getResponseHeaders";
    type Args = NoArgs;
    type Reply = response::GetResponseHeaders;
}

/// `getResponseContent` method marker.
pub struct GetResponseContent;
impl sealed::Sealed for GetResponseContent {}
impl Method for GetResponseContent {
    const NAME: &'static str = "getResponseContent";
    type Args = NoArgs;
    type Reply = response::GetResponseContent;
}

/// `getSecurityInfo` method marker.
pub struct GetSecurityInfo;
impl sealed::Sealed for GetSecurityInfo {}
impl Method for GetSecurityInfo {
    const NAME: &'static str = "getSecurityInfo";
    type Args = NoArgs;
    type Reply = response::GetSecurityInfo;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn get_request_headers_response_deserializes() {
        let v = json!({
            "from": "server1.conn0.netEvent6",
            "headers": [
                {"name": "Accept", "value": "text/html"},
                {"name": "User-Agent", "value": "Mozilla/5.0"}
            ],
            "headersSize": 120
        });
        let reply: response::GetRequestHeaders = serde_json::from_value(v).unwrap();
        assert_eq!(reply.headers.len(), 2);
        assert_eq!(reply.headers[0].name, "Accept");
        assert_eq!(reply.headers[0].value.preview(), "text/html");
        assert_eq!(reply.headers_size, 120);
    }

    #[test]
    fn get_request_headers_longstring_value_deserializes() {
        let v = json!({
            "from": "server1.conn0.netEvent6",
            "headers": [{
                "name": "Set-Cookie",
                "value": {
                    "type": "longString",
                    "actor": "conn0/longString1",
                    "length": 50000,
                    "initial": "session=abc123"
                }
            }],
            "headersSize": 200
        });
        let reply: response::GetRequestHeaders = serde_json::from_value(v).unwrap();
        assert_eq!(reply.headers.len(), 1);
        assert_eq!(reply.headers[0].name, "Set-Cookie");
        assert!(reply.headers[0].value.is_actor());
        assert_eq!(reply.headers[0].value.preview(), "session=abc123");
    }

    #[test]
    fn get_response_headers_response_deserializes() {
        let v = json!({
            "from": "server1.conn0.netEvent6",
            "headers": [{"name": "Content-Type", "value": "text/html; charset=utf-8"}],
            "headersSize": 48
        });
        let reply: response::GetResponseHeaders = serde_json::from_value(v).unwrap();
        assert_eq!(reply.headers.len(), 1);
        assert_eq!(reply.headers[0].name, "Content-Type");
    }

    #[test]
    fn get_response_content_response_deserializes() {
        let v = json!({
            "from": "server1.conn0.netEvent6",
            "content": {"text": "hello", "mimeType": "text/html", "size": 5}
        });
        let reply: response::GetResponseContent = serde_json::from_value(v).unwrap();
        assert!(reply.content.is_some());
    }

    #[test]
    fn no_args_serializes_to_empty_object() {
        let v = serde_json::to_value(NoArgs {}).unwrap();
        assert!(v.as_object().is_some_and(serde_json::Map::is_empty));
    }

    #[test]
    fn method_names_are_correct() {
        assert_eq!(GetRequestHeaders::NAME, "getRequestHeaders");
        assert_eq!(GetResponseHeaders::NAME, "getResponseHeaders");
        assert_eq!(GetResponseContent::NAME, "getResponseContent");
        assert_eq!(GetSecurityInfo::NAME, "getSecurityInfo");
    }

    #[test]
    fn get_security_info_response_deserializes_https() {
        let v = json!({
            "from": "server1.conn0.netEvent6",
            "securityInfo": {
                "state": "secure",
                "protocolVersion": "TLSv1.3",
                "cipherSuite": "TLS_AES_128_GCM_SHA256",
                "weaknessReasons": [],
                "hsts": true,
                "cert": {
                    "subject": {"commonName": "example.com"},
                    "issuer": {"commonName": "DigiCert TLS RSA SHA256 2020 CA1"},
                    "validity": {"start": "…", "end": "…"},
                    "fingerprint": {"sha256": "AA:BB"}
                }
            }
        });
        let reply: response::GetSecurityInfo = serde_json::from_value(v).unwrap();
        assert!(reply.security_info.is_some());
        assert_eq!(reply.security_info.unwrap()["protocolVersion"], "TLSv1.3");
    }

    #[test]
    fn get_security_info_response_deserializes_http_null() {
        // Plain-HTTP requests report no security info.
        let v = json!({
            "from": "server1.conn0.netEvent7",
            "securityInfo": null
        });
        let reply: response::GetSecurityInfo = serde_json::from_value(v).unwrap();
        assert!(reply.security_info.is_none());
    }
}
