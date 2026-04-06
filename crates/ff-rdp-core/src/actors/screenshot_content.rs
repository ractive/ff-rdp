use serde_json::{Value, json};

use crate::actor::actor_request;
use crate::error::ProtocolError;
use crate::transport::RdpTransport;

/// Operations on the ScreenshotContentActor (available per-tab for screenshots).
pub struct ScreenshotContentActor;

impl ScreenshotContentActor {
    /// Capture a screenshot of the current page via the RDP ScreenshotContentActor.
    ///
    /// Returns the screenshot data as a `data:image/png;base64,...` string.
    pub fn capture(
        transport: &mut RdpTransport,
        actor: &str,
    ) -> Result<ScreenshotCapture, ProtocolError> {
        let params = json!({
            "fullPage": false,
            "ratio": 1.0,
        });
        let response = actor_request(transport, actor, "captureScreenshot", Some(&params))?;

        // The response shape may be either:
        //   { "capture": { "data": "data:...", "width": N, "height": N } }
        // or directly:
        //   { "data": "...", "width": N, "height": N }
        let capture = response.get("capture").unwrap_or(&response);

        let data = capture
            .get("data")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ProtocolError::InvalidPacket(
                    "captureScreenshot response missing 'data' field".into(),
                )
            })?
            .to_owned();

        Ok(ScreenshotCapture { data })
    }
}

/// Result of a successful screenshot capture.
pub struct ScreenshotCapture {
    /// The screenshot as a data URL (`data:image/png;base64,...`).
    pub data: String,
}
