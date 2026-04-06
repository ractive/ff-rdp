use std::time::Duration;

use serde_json::Value;

use crate::error::ProtocolError;
use crate::transport::RdpTransport;

/// High-level connection to a Firefox RDP server.
///
/// Wraps [`RdpTransport`] and handles the initial handshake (greeting
/// validation). All actor operations go through the underlying transport
/// which is accessible via [`transport_mut`](Self::transport_mut).
#[derive(Debug)]
pub struct RdpConnection {
    transport: RdpTransport,
    timeout: Duration,
}

impl RdpConnection {
    /// Connect to Firefox, read the greeting, and validate `applicationType`.
    ///
    /// The read timeout configured on the socket handles the greeting timeout.
    pub fn connect(host: &str, port: u16, timeout: Duration) -> Result<Self, ProtocolError> {
        let mut transport = RdpTransport::connect_raw(host, port, timeout)?;

        let greeting = transport.recv()?;

        Self::validate_greeting(&greeting)?;

        Ok(Self { transport, timeout })
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
}
