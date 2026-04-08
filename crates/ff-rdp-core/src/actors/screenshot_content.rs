use serde_json::{Value, json};

use crate::actor::actor_request;
use crate::error::ProtocolError;
use crate::transport::RdpTransport;

/// Method names tried in order when sending a capture request to the
/// `screenshotContentActor`.  Firefox has renamed this method across versions:
///
/// - `captureScreenshot` — Firefox < 149 (original name)
/// - `screenshot`        — Firefox 149+ candidate fallback
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
