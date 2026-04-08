use serde_json::{Value, json};

use crate::actor::actor_request;
use crate::error::ProtocolError;
use crate::transport::RdpTransport;

/// Method names tried in order when sending a capture request to the
/// `screenshotContentActor`.  Firefox has renamed this method across versions:
///
/// - `captureScreenshot` — Firefox < 87 (original name)
/// - `screenshot`        — intermediate fallback
/// - `capture`           — additional fallback for future renames
const CAPTURE_METHODS: &[&str] = &["captureScreenshot", "screenshot", "capture"];

/// Operations on the ScreenshotContentActor (available per-tab for screenshots).
pub struct ScreenshotContentActor;

impl ScreenshotContentActor {
    /// Capture a screenshot of the current page via the RDP ScreenshotContentActor.
    ///
    /// Tries each method name in [`CAPTURE_METHODS`] in order, falling back to the
    /// next when Firefox returns `unrecognizedPacketType`.  Returns the screenshot
    /// data as a `data:image/png;base64,...` string.
    ///
    /// This is the **legacy** single-step method.  Firefox 149+ replaced this
    /// with the two-step [`prepareCapture`](Self::prepareCapture) / root
    /// `screenshotActor.capture` protocol.
    pub fn capture(
        transport: &mut RdpTransport,
        actor: &str,
    ) -> Result<ScreenshotCapture, ProtocolError> {
        let params = json!({
            "fullPage": false,
            "ratio": 1.0,
        });

        let mut last_err: Option<ProtocolError> = None;
        for &method in CAPTURE_METHODS {
            match actor_request(transport, actor, method, Some(&params)) {
                Ok(response) => {
                    return extract_capture_data(&response, method);
                }
                Err(e) if e.is_unrecognized_packet_type() => {
                    // This method name is not known to the actor — try the next one.
                    last_err = Some(e);
                }
                Err(e) => return Err(e),
            }
        }

        // All method names were unrecognised — return the last unrecognized error.
        Err(last_err.expect("CAPTURE_METHODS is non-empty"))
    }

    /// Prepare viewport metadata for the Firefox 149+ two-step screenshot protocol.
    ///
    /// Firefox 149 split screenshot capture into two actors:
    /// 1. (Content process) `screenshotContentActor.prepareCapture` → collects
    ///    viewport rect, device pixel ratio, and zoom level.
    /// 2. (Parent process) `screenshotActor.capture` on the root actor → performs
    ///    the actual `browsingContext.drawSnapshot` and returns the PNG data URL.
    ///
    /// This method handles step 1.  Pass the returned [`PrepareCapture`] value to
    /// [`ScreenshotActor::capture`] to complete step 2.
    pub fn prepare_capture(
        transport: &mut RdpTransport,
        actor: &str,
        full_page: bool,
    ) -> Result<PrepareCapture, ProtocolError> {
        let params = json!({ "args": { "fullpage": full_page } });
        let response = actor_request(transport, actor, "prepareCapture", Some(&params))?;

        // Firefox wraps the result in a `value` envelope: `{ "value": { ... } }`
        let value = response.get("value").unwrap_or(&response);

        let window_dpr = value
            .get("windowDpr")
            .and_then(Value::as_f64)
            .unwrap_or(1.0);
        let window_zoom = value
            .get("windowZoom")
            .and_then(Value::as_f64)
            .unwrap_or(1.0);

        let rect = value.get("rect").and_then(|r| {
            if r.is_null() {
                None
            } else {
                Some(CaptureRect {
                    left: r.get("left")?.as_f64()?,
                    top: r.get("top")?.as_f64()?,
                    width: r.get("width")?.as_f64()?,
                    height: r.get("height")?.as_f64()?,
                })
            }
        });

        Ok(PrepareCapture {
            window_dpr,
            window_zoom,
            rect,
        })
    }
}

/// Extract the `data:image/png;base64,...` string from a capture response.
///
/// The response shape may be either:
///   `{ "capture": { "data": "data:...", "width": N, "height": N } }`
/// or directly:
///   `{ "data": "...", "width": N, "height": N }`
fn extract_capture_data(
    response: &Value,
    method: &str,
) -> Result<ScreenshotCapture, ProtocolError> {
    let capture = response.get("capture").unwrap_or(response);

    let data = capture
        .get("data")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ProtocolError::InvalidPacket(format!("{method} response missing 'data' field"))
        })?
        .to_owned();

    Ok(ScreenshotCapture { data })
}

/// Result of a successful screenshot capture.
#[derive(Debug)]
pub struct ScreenshotCapture {
    /// The screenshot as a data URL (`data:image/png;base64,...`).
    pub data: String,
}

/// Viewport metadata returned by [`ScreenshotContentActor::prepare_capture`].
///
/// Used as input for the second step of the Firefox 149+ screenshot protocol:
/// [`ScreenshotActor::capture`](crate::actors::screenshot::ScreenshotActor::capture).
#[derive(Debug, Clone)]
pub struct PrepareCapture {
    /// Device pixel ratio of the window.
    pub window_dpr: f64,
    /// Current zoom level of the window.
    pub window_zoom: f64,
    /// Capture region returned by `prepareCapture`.  `None` for viewport-only
    /// captures; `Some(...)` for full-page or element captures.
    pub rect: Option<CaptureRect>,
}

/// Capture region as returned by `screenshotContentActor.prepareCapture`.
#[derive(Debug, Clone)]
pub struct CaptureRect {
    /// Left edge of the capture region in CSS pixels.
    pub left: f64,
    /// Top edge of the capture region in CSS pixels.
    pub top: f64,
    /// Width of the capture region in CSS pixels.
    pub width: f64,
    /// Height of the capture region in CSS pixels.
    pub height: f64,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn extract_capture_data_direct_data_field() {
        let response = json!({"data": "data:image/png;base64,abc123", "width": 800, "height": 600});
        let capture = extract_capture_data(&response, "captureScreenshot").unwrap();
        assert_eq!(capture.data, "data:image/png;base64,abc123");
    }

    #[test]
    fn extract_capture_data_nested_capture_field() {
        let response = json!({
            "capture": {"data": "data:image/png;base64,xyz789", "width": 1024, "height": 768}
        });
        let capture = extract_capture_data(&response, "screenshot").unwrap();
        assert_eq!(capture.data, "data:image/png;base64,xyz789");
    }

    #[test]
    fn extract_capture_data_missing_data_returns_error() {
        let response = json!({"width": 800, "height": 600});
        let err = extract_capture_data(&response, "captureScreenshot").unwrap_err();
        assert!(
            err.to_string().contains("captureScreenshot"),
            "error should mention method name: {err}"
        );
    }

    #[test]
    fn capture_methods_is_non_empty() {
        assert!(!CAPTURE_METHODS.is_empty());
        assert_eq!(CAPTURE_METHODS[0], "captureScreenshot");
    }

    // --- prepare_capture parsing ---

    /// Helper: build a PrepareCapture by parsing a fake `prepareCapture` response.
    fn parse_prepare_capture(response: serde_json::Value) -> PrepareCapture {
        use std::io::{BufReader, Write as _};
        use std::net::{TcpListener, TcpStream};

        use crate::transport::{RdpTransport, encode_frame, recv_from};

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let client = TcpStream::connect(addr).unwrap();
        let (server, _) = listener.accept().unwrap();

        // Send the actor request from a thread so the server side can reply.
        let t = std::thread::spawn(move || {
            let mut reader = BufReader::new(&server);
            let _req = recv_from(&mut reader).unwrap();
            let frame = encode_frame(&serde_json::to_string(&response).unwrap());
            (&server).write_all(frame.as_bytes()).unwrap();
        });

        let writer = client.try_clone().unwrap();
        let reader = BufReader::new(client);
        let mut transport = RdpTransport::from_parts(reader, writer);

        let result = ScreenshotContentActor::prepare_capture(
            &mut transport,
            "server1.conn0.screenshotContentActor1",
            false,
        )
        .unwrap();
        t.join().unwrap();
        result
    }

    #[test]
    fn prepare_capture_parses_rect_when_present() {
        let response = serde_json::json!({
            "from": "server1.conn0.screenshotContentActor1",
            "value": {
                "windowDpr": 2.0,
                "windowZoom": 1.5,
                "rect": {
                    "left": 10.0,
                    "top": 20.0,
                    "width": 800.0,
                    "height": 600.0
                },
                "messages": []
            }
        });

        let prep = parse_prepare_capture(response);
        assert!((prep.window_dpr - 2.0).abs() < f64::EPSILON);
        assert!((prep.window_zoom - 1.5).abs() < f64::EPSILON);

        let rect = prep.rect.expect("rect should be Some when non-null");
        assert!((rect.left - 10.0).abs() < f64::EPSILON);
        assert!((rect.top - 20.0).abs() < f64::EPSILON);
        assert!((rect.width - 800.0).abs() < f64::EPSILON);
        assert!((rect.height - 600.0).abs() < f64::EPSILON);
    }

    #[test]
    fn prepare_capture_returns_none_rect_when_null() {
        let response = serde_json::json!({
            "from": "server1.conn0.screenshotContentActor1",
            "value": {
                "windowDpr": 1.0,
                "windowZoom": 1.0,
                "rect": null,
                "messages": []
            }
        });

        let prep = parse_prepare_capture(response);
        assert!(
            prep.rect.is_none(),
            "rect should be None when response contains null"
        );
    }

    #[test]
    fn prepare_capture_returns_none_rect_when_field_absent() {
        let response = serde_json::json!({
            "from": "server1.conn0.screenshotContentActor1",
            "value": {
                "windowDpr": 1.0,
                "windowZoom": 1.0,
                "messages": []
            }
        });

        let prep = parse_prepare_capture(response);
        assert!(
            prep.rect.is_none(),
            "rect should be None when field is absent"
        );
    }
}
