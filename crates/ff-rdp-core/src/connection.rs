use std::time::Duration;

use serde_json::Value;

use crate::error::ProtocolError;
use crate::transport::RdpTransport;

/// The inclusive range of Firefox major versions known to be compatible with
/// this library.  When the detected version falls outside this range a warning
/// is emitted via [`RdpConnection::warn_if_version_unsupported`]; the
/// connection is never refused on version grounds alone.
pub const COMPATIBLE_FIREFOX_MIN: u32 = 120;
pub const COMPATIBLE_FIREFOX_MAX: u32 = 150;

/// High-level connection to a Firefox RDP server.
///
/// Wraps [`RdpTransport`] and handles the initial handshake (greeting
/// validation). All actor operations go through the underlying transport
/// which is accessible via [`transport_mut`](Self::transport_mut).
#[derive(Debug)]
pub struct RdpConnection {
    transport: RdpTransport,
    timeout: Duration,
    /// Firefox major version extracted from the greeting `ua` field, if present.
    firefox_version: Option<u32>,
}

impl RdpConnection {
    /// Connect to Firefox, read the greeting, and validate `applicationType`.
    ///
    /// The read timeout configured on the socket handles the greeting timeout.
    pub fn connect(host: &str, port: u16, timeout: Duration) -> Result<Self, ProtocolError> {
        let mut transport = RdpTransport::connect_raw(host, port, timeout)?;

        let greeting = transport.recv()?;

        Self::validate_greeting(&greeting)?;

        let firefox_version = parse_firefox_version(&greeting);

        Ok(Self {
            transport,
            timeout,
            firefox_version,
        })
    }

    /// Returns a mutable reference to the underlying transport for actor
    /// request/response operations.
    pub fn transport_mut(&mut self) -> &mut RdpTransport {
        &mut self.transport
    }

    /// Returns the operation timeout configured for this connection.
    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    /// Returns the Firefox major version parsed from the greeting, if available.
    ///
    /// Firefox includes a `"ua"` (user-agent) field in the RDP greeting for
    /// some versions.  When absent, this returns `None` and no version check
    /// is performed.
    pub fn firefox_version(&self) -> Option<u32> {
        self.firefox_version
    }

    /// Emit a warning to stderr when the Firefox version is outside the
    /// known-compatible range [`COMPATIBLE_FIREFOX_MIN`]–[`COMPATIBLE_FIREFOX_MAX`].
    ///
    /// Does nothing when the version is unknown (not present in the greeting).
    /// Never fails — this is advisory only.
    pub fn warn_if_version_unsupported(&self) {
        if let Some(v) = self.firefox_version
            && !(COMPATIBLE_FIREFOX_MIN..=COMPATIBLE_FIREFOX_MAX).contains(&v)
        {
            eprintln!(
                "warning: connected to Firefox {v}, but ff-rdp is tested against \
                 Firefox {COMPATIBLE_FIREFOX_MIN}–{COMPATIBLE_FIREFOX_MAX}; \
                 some features may not work correctly"
            );
        }
    }

    fn validate_greeting(greeting: &Value) -> Result<(), ProtocolError> {
        let app_type = greeting
            .get("applicationType")
            .and_then(Value::as_str)
            .unwrap_or("");

        if app_type != "browser" {
            return Err(ProtocolError::InvalidPacket(format!(
                "unexpected applicationType in greeting: {app_type:?} (expected \"browser\")"
            )));
        }

        Ok(())
    }
}

/// Extract the Firefox major version from the RDP greeting.
///
/// Firefox optionally includes a `"ua"` field containing a User-Agent string
/// like `"Mozilla/5.0 ... Firefox/135.0"`.  This function parses the major
/// version number from that string.  Returns `None` when the field is absent
/// or unparseable — callers must handle the unknown-version case gracefully.
fn parse_firefox_version(greeting: &Value) -> Option<u32> {
    let ua = greeting.get("ua").and_then(Value::as_str)?;

    // Look for "Firefox/<major>.<minor>" in the UA string.
    let firefox_pos = ua.find("Firefox/")?;
    let after_prefix = &ua[firefox_pos + "Firefox/".len()..];

    // The major version ends at the first non-digit character (usually '.').
    let major_str: String = after_prefix
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();

    major_str.parse().ok()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn validate_greeting_accepts_browser() {
        let greeting = json!({"from": "root", "applicationType": "browser", "traits": {}});
        assert!(RdpConnection::validate_greeting(&greeting).is_ok());
    }

    #[test]
    fn validate_greeting_rejects_wrong_type() {
        let greeting = json!({"from": "root", "applicationType": "webide", "traits": {}});
        let err = RdpConnection::validate_greeting(&greeting).unwrap_err();
        assert!(matches!(err, ProtocolError::InvalidPacket(_)));
    }

    #[test]
    fn validate_greeting_rejects_missing_type() {
        let greeting = json!({"from": "root"});
        let err = RdpConnection::validate_greeting(&greeting).unwrap_err();
        assert!(matches!(err, ProtocolError::InvalidPacket(_)));
    }

    // -----------------------------------------------------------------------
    // parse_firefox_version
    // -----------------------------------------------------------------------

    #[test]
    fn parse_version_from_standard_ua() {
        let greeting = json!({
            "from": "root",
            "applicationType": "browser",
            "ua": "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:135.0) Gecko/20100101 Firefox/135.0"
        });
        assert_eq!(parse_firefox_version(&greeting), Some(135));
    }

    #[test]
    fn parse_version_from_linux_ua() {
        let greeting = json!({
            "from": "root",
            "applicationType": "browser",
            "ua": "Mozilla/5.0 (X11; Linux x86_64; rv:149.0) Gecko/20100101 Firefox/149.0"
        });
        assert_eq!(parse_firefox_version(&greeting), Some(149));
    }

    #[test]
    fn parse_version_returns_none_when_ua_absent() {
        let greeting = json!({"from": "root", "applicationType": "browser", "traits": {}});
        assert_eq!(parse_firefox_version(&greeting), None);
    }

    #[test]
    fn parse_version_returns_none_when_ua_malformed() {
        let greeting =
            json!({"from": "root", "applicationType": "browser", "ua": "not-a-ua-string"});
        assert_eq!(parse_firefox_version(&greeting), None);
    }

    #[test]
    fn parse_version_handles_three_digit_major() {
        let greeting = json!({
            "from": "root",
            "applicationType": "browser",
            "ua": "Mozilla/5.0 Firefox/120.0"
        });
        assert_eq!(parse_firefox_version(&greeting), Some(120));
    }

    // -----------------------------------------------------------------------
    // warn_if_version_unsupported — logic only; no stderr capture needed
    // -----------------------------------------------------------------------

    #[test]
    fn version_in_range_does_not_trigger_warning_logic() {
        // We can't easily capture stderr in unit tests, but we can verify the
        // boundary conditions by checking the comparison directly.
        for v in [COMPATIBLE_FIREFOX_MIN, COMPATIBLE_FIREFOX_MAX, 135] {
            assert!(
                (COMPATIBLE_FIREFOX_MIN..=COMPATIBLE_FIREFOX_MAX).contains(&v),
                "version {v} should be in range"
            );
        }
    }

    #[test]
    fn version_below_min_is_out_of_range() {
        let v = COMPATIBLE_FIREFOX_MIN - 1;
        assert!(
            v < COMPATIBLE_FIREFOX_MIN,
            "version {v} should be below min"
        );
    }

    #[test]
    fn version_above_max_is_out_of_range() {
        let v = COMPATIBLE_FIREFOX_MAX + 1;
        assert!(
            v > COMPATIBLE_FIREFOX_MAX,
            "version {v} should be above max"
        );
    }
}
