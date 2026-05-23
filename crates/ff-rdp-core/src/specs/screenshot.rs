//! Spec for the Screenshot actor (root-level, Firefox 87+).
//!
//! Mirrors <https://searchfox.org/mozilla-central/source/devtools/shared/specs/screenshot.js>
//!
//! The `capture` method takes a nested `args` object; the spec struct mirrors that shape.

use serde::{Deserialize, Serialize};

use super::{Method, sealed};

// ---------------------------------------------------------------------------
// Request args
// ---------------------------------------------------------------------------

pub mod request {
    use super::Serialize;

    /// Optional capture rect (for full-page / element screenshots).
    #[derive(Debug, Clone, Serialize)]
    pub struct CaptureRect {
        pub left: f64,
        pub top: f64,
        pub width: f64,
        pub height: f64,
    }

    /// Inner args object passed inside the outer `{ "args": {...} }` wrapper.
    #[derive(Debug, Clone, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct CaptureArgs {
        /// Firefox expects the wire key `browsingContextID` (uppercase ID).
        #[serde(rename = "browsingContextID")]
        pub browsing_context_id: u64,
        pub fullpage: bool,
        pub dpr: f64,
        pub snapshot_scale: f64,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub rect: Option<CaptureRect>,
    }

    /// Top-level wrapper — Firefox expects `{ "args": { ... } }`.
    #[derive(Debug, Clone, Serialize)]
    pub struct Capture {
        pub args: CaptureArgs,
    }
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

pub mod response {
    use super::Deserialize;

    /// Inner value returned by `capture`.
    #[derive(Debug, Clone, Default, Deserialize)]
    pub struct CaptureValue {
        /// The data URL (e.g. `data:image/png;base64,...`).
        pub data: String,
        #[serde(default)]
        pub filename: String,
    }

    /// Reply for `capture`.
    #[derive(Debug, Clone, Default, Deserialize)]
    pub struct Capture {
        /// The capture result is nested under `"value"`.
        #[serde(default)]
        pub value: Option<CaptureValue>,
    }
}

// ---------------------------------------------------------------------------
// Method markers
// ---------------------------------------------------------------------------

/// `capture` method marker.
pub struct Capture;
impl sealed::Sealed for Capture {}
impl Method for Capture {
    const NAME: &'static str = "capture";
    type Args = request::Capture;
    type Reply = response::Capture;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn capture_request_serializes_browsing_context_id() {
        let args = request::Capture {
            args: request::CaptureArgs {
                browsing_context_id: 42,
                fullpage: false,
                dpr: 1.0,
                snapshot_scale: 1.0,
                rect: None,
            },
        };
        let v = serde_json::to_value(&args).unwrap();
        assert_eq!(v["args"]["browsingContextID"], 42);
        assert_eq!(v["args"]["fullpage"], false);
        assert_eq!(v["args"]["dpr"], 1.0);
        assert!(v["args"].get("rect").is_none());
    }

    #[test]
    fn capture_request_serializes_rect_when_present() {
        let args = request::Capture {
            args: request::CaptureArgs {
                browsing_context_id: 1,
                fullpage: true,
                dpr: 2.0,
                snapshot_scale: 2.0,
                rect: Some(request::CaptureRect {
                    left: 0.0,
                    top: 0.0,
                    width: 800.0,
                    height: 600.0,
                }),
            },
        };
        let v = serde_json::to_value(&args).unwrap();
        assert_eq!(v["args"]["rect"]["width"], 800.0);
    }

    #[test]
    fn capture_response_deserializes_data_url() {
        let v = json!({
            "from": "server1.conn0.screenshotActor7",
            "value": {
                "data": "data:image/png;base64,abc123",
                "filename": "screenshot.png"
            }
        });
        let reply: response::Capture = serde_json::from_value(v).unwrap();
        let val = reply.value.expect("value should be present");
        assert_eq!(val.data, "data:image/png;base64,abc123");
        assert_eq!(val.filename, "screenshot.png");
    }

    #[test]
    fn method_name_is_correct() {
        assert_eq!(Capture::NAME, "capture");
    }
}
