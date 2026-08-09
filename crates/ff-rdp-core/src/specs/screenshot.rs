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
    ///
    /// # Non-spec fields
    ///
    /// The following three fields are **not** declared in the canonical Firefox
    /// spec dict at `devtools/shared/specs/screenshot.js:13-20` but are read
    /// directly by the server actor
    /// (`devtools/server/actors/utils/capture-screenshot.js`):
    ///
    /// - `browsingContextID` — selects the browsing context to capture.
    /// - `rect` — optional crop rectangle in CSS pixels.
    /// - `snapshotScale` — optional scale factor; server defaults to `1.0`
    ///   when absent (treated as `None`).
    #[derive(Debug, Clone, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct CaptureArgs {
        /// Firefox expects the wire key `browsingContextID` (uppercase ID).
        ///
        /// Non-spec field: not in `devtools/shared/specs/screenshot.js`.
        #[serde(rename = "browsingContextID")]
        pub browsing_context_id: u64,
        pub fullpage: bool,
        /// Device pixel ratio — per Firefox spec this is a string (e.g. `"2.0"`).
        ///
        /// Pass `Some("2.0".to_string())` to request a 2x DPR capture.
        /// `None` omits the field and lets Firefox use the display DPR.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub dpr: Option<String>,
        /// Snapshot scale factor — typically equal to the DPR value as a float.
        ///
        /// Non-spec field: not in `devtools/shared/specs/screenshot.js`.
        ///
        /// **There is no server-side default.**
        /// `devtools/server/actors/utils/capture-screenshot.js` reads
        /// `const ratio = args.snapshotScale;` and passes it verbatim to
        /// `drawSnapshot`; omitting it makes the resulting canvas `NaN`-sized
        /// and the reply comes back with `data: null` (iter-135).  Callers
        /// should always populate this with `windowDpr * windowZoom`; `None`
        /// is retained only so the struct can mirror an omitted-field packet
        /// in tests.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub snapshot_scale: Option<f64>,
        /// Optional delay in milliseconds before capturing — per Firefox spec.
        ///
        /// Useful for waiting for animations or deferred renders to settle.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub delay: Option<String>,
        /// Non-spec field: not in `devtools/shared/specs/screenshot.js`.
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

    /// A single user-facing diagnostic emitted by the server-side capture.
    ///
    /// `capture-screenshot.js` pushes entries here for a decreased DPR, a
    /// truncated oversized page, and — crucially — for a rendering failure,
    /// in which case `data` comes back `null`.
    #[derive(Debug, Clone, Default, Deserialize)]
    pub struct CaptureMessage {
        /// `"error"`, `"warn"`, or `"info"`.
        #[serde(default)]
        pub level: String,
        /// Localised message text (the server localises to the browser's UI
        /// locale, so never match on its contents).
        #[serde(default)]
        pub text: String,
    }

    /// Inner value returned by `capture`.
    #[derive(Debug, Clone, Default, Deserialize)]
    pub struct CaptureValue {
        /// The data URL (e.g. `data:image/png;base64,...`).
        ///
        /// `null` when the server-side render failed — see
        /// [`CaptureValue::messages`] for why.  It is **not** optional in the
        /// spec's `RetVal("json")` sense; Firefox simply always includes the
        /// key and sets it to `null` on failure.
        #[serde(default)]
        pub data: Option<String>,
        #[serde(default)]
        pub filename: String,
        /// Server-side diagnostics.  Empty on a clean capture.
        #[serde(default)]
        pub messages: Vec<CaptureMessage>,
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
                dpr: None,
                snapshot_scale: None,
                delay: None,
                rect: None,
            },
        };
        let v = serde_json::to_value(&args).unwrap();
        assert_eq!(v["args"]["browsingContextID"], 42);
        assert_eq!(v["args"]["fullpage"], false);
        // dpr and snapshotScale are None so they must be omitted.
        assert!(v["args"].get("dpr").is_none());
        assert!(v["args"].get("snapshotScale").is_none());
        assert!(v["args"].get("rect").is_none());
    }

    #[test]
    fn capture_request_serializes_snapshot_scale_when_present() {
        let args = request::Capture {
            args: request::CaptureArgs {
                browsing_context_id: 1,
                fullpage: false,
                dpr: None,
                snapshot_scale: Some(2.0),
                delay: None,
                rect: None,
            },
        };
        let v = serde_json::to_value(&args).unwrap();
        assert_eq!(v["args"]["snapshotScale"], 2.0);
    }

    #[test]
    fn capture_request_serializes_dpr_as_string() {
        let args = request::Capture {
            args: request::CaptureArgs {
                browsing_context_id: 1,
                fullpage: false,
                dpr: Some("2.0".to_string()),
                snapshot_scale: Some(2.0),
                delay: None,
                rect: None,
            },
        };
        let v = serde_json::to_value(&args).unwrap();
        assert_eq!(v["args"]["dpr"], "2.0");
    }

    #[test]
    fn capture_request_serializes_delay_when_present() {
        let args = request::Capture {
            args: request::CaptureArgs {
                browsing_context_id: 1,
                fullpage: false,
                dpr: None,
                snapshot_scale: None,
                delay: Some("500".to_string()),
                rect: None,
            },
        };
        let v = serde_json::to_value(&args).unwrap();
        assert_eq!(v["args"]["delay"], "500");
    }

    #[test]
    fn capture_request_serializes_rect_when_present() {
        let args = request::Capture {
            args: request::CaptureArgs {
                browsing_context_id: 1,
                fullpage: true,
                dpr: Some("2.0".to_string()),
                snapshot_scale: Some(2.0),
                delay: None,
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
        assert_eq!(val.data.as_deref(), Some("data:image/png;base64,abc123"));
        assert_eq!(val.filename, "screenshot.png");
    }

    /// iter-135: the Firefox 153 failure reply must deserialise.  With the
    /// pre-135 `data: String` this returned a serde error ("invalid type: null")
    /// and the server's explanation never reached the user.
    #[test]
    fn capture_response_deserializes_null_data_with_messages() {
        let v = json!({
            "from": "server1.conn0.screenshotActor7",
            "value": {
                "data": null,
                "filename": "Bildschirmfoto.png",
                "messages": [{"level": "error", "text": "Fehler beim Erstellen der Grafik."}]
            }
        });
        let reply: response::Capture = serde_json::from_value(v).unwrap();
        let val = reply.value.expect("value should be present");
        assert!(val.data.is_none(), "null data must deserialise to None");
        assert_eq!(val.messages.len(), 1);
        assert_eq!(val.messages[0].level, "error");
        assert!(val.messages[0].text.starts_with("Fehler"));
    }

    #[test]
    fn method_name_is_correct() {
        assert_eq!(Capture::NAME, "capture");
    }
}
