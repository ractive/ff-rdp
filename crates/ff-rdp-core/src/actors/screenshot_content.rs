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

        Ok(PrepareCapture {
            window_dpr,
            window_zoom,
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
}
