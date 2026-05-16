//! Firefox RDP `deviceActor` — queries runtime version information.
//!
//! The device actor is returned via `root.getRoot` → `deviceActor`.  Its
//! `getDescription` method returns a description block that includes
//! `appVersion` (e.g. `"137.0"`) and `platformVersion`.  This is useful as a
//! fallback when the RDP greeting's `ua` field is absent (some Firefox builds
//! omit it).

use serde_json::{Value, json};

use crate::actor::actor_request;
use crate::error::ProtocolError;
use crate::transport::RdpTransport;

/// Operations on the Firefox RDP device actor.
pub struct DeviceActor;

impl DeviceActor {
    /// Obtain the device actor ID from the root `getRoot` response.
    ///
    /// Returns `None` when `getRoot` does not include a `deviceActor` field
    /// (pre-87 Firefox or stripped builds).
    pub fn get_actor_id(transport: &mut RdpTransport) -> Result<Option<String>, ProtocolError> {
        // Send `getRoot` directly so we can filter push events from the root
        // actor (e.g. `tabListChanged` with a `type` field) without confusing
        // them with the actual reply — the generic `actor_request` matches the
        // first packet whose `from` equals `"root"`, which would happily
        // accept a push event as the response.
        let request = json!({"to": "root", "type": "getRoot"});
        transport.send(&request)?;

        let response = loop {
            let msg = transport.recv()?;
            let from = msg.get("from").and_then(Value::as_str).unwrap_or_default();
            if from == "root" {
                if msg.get("type").is_some() {
                    // Push event — skip.
                    continue;
                }
                break msg;
            }
        };

        if let Some(err) = response.get("error").and_then(Value::as_str) {
            return Err(ProtocolError::ActorError {
                actor: "root".to_owned(),
                kind: crate::error::ActorErrorKind::from_code(err),
                error: err.to_owned(),
                message: response
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
            });
        }

        let id = response
            .get("deviceActor")
            .and_then(Value::as_str)
            .map(str::to_owned);

        Ok(id)
    }

    /// Call `getDescription` on the device actor and extract the Firefox major
    /// version number from the `appVersion` field.
    ///
    /// Returns `None` when the actor does not advertise `appVersion` or when
    /// the value cannot be parsed as a major version number.
    pub fn query_firefox_version(
        transport: &mut RdpTransport,
        actor_id: &str,
    ) -> Result<Option<u32>, ProtocolError> {
        let response = actor_request(transport, actor_id, "getDescription", None)?;

        let version = parse_app_version(&response);
        Ok(version)
    }

    /// Convenience wrapper: probe `getRoot` for the device actor then call
    /// `getDescription` to retrieve the Firefox major version.
    ///
    /// Returns `None` when either step fails to produce a version — the caller
    /// should treat this as "version unknown" and not gate functionality on it.
    ///
    /// # Errors
    ///
    /// Returns `Err` only for transport-level failures (socket errors, malformed
    /// packets).  A missing actor or absent `appVersion` field yields `Ok(None)`.
    pub fn query_version(transport: &mut RdpTransport) -> Result<Option<u32>, ProtocolError> {
        // Treat actor-level errors (e.g. `unknownActor`, `noSuchActor`) as
        // "version unknown" rather than propagating — this matches the
        // documented contract that callers can treat failures as a missing
        // version.  Transport-level errors still propagate so a broken socket
        // surfaces up the call stack.
        let actor_id = match Self::get_actor_id(transport) {
            Ok(Some(id)) => id,
            Ok(None) | Err(ProtocolError::ActorError { .. }) => return Ok(None),
            Err(e) => return Err(e),
        };
        match Self::query_firefox_version(transport, &actor_id) {
            Ok(v) => Ok(v),
            Err(ProtocolError::ActorError { .. }) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

/// Parse a Firefox major version number from the `appVersion` field of a
/// `getDescription` response.
///
/// `appVersion` is typically `"137.0"` or `"137.0a1"`.  We extract the
/// leading digit sequence before the first `.` or non-digit character.
fn parse_app_version(response: &Value) -> Option<u32> {
    let description = response.get("value").unwrap_or(response);

    let app_version = description
        .get("appVersion")
        .or_else(|| description.get("platformVersion"))
        .and_then(Value::as_str)?;

    let major: String = app_version
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();

    major.parse().ok()
}

#[cfg(test)]
mod tests {
    use std::io::BufReader;
    use std::net::{TcpListener, TcpStream};

    use serde_json::json;

    use super::*;
    use crate::transport::{RdpTransport, encode_frame, recv_from};

    fn make_transport_pair() -> (RdpTransport, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let client = TcpStream::connect(addr).unwrap();
        let (server, _) = listener.accept().unwrap();
        let writer = client.try_clone().unwrap();
        let reader = BufReader::new(client);
        (RdpTransport::from_parts(reader, writer), server)
    }

    #[allow(clippy::needless_pass_by_value)]
    fn server_reply(server: &TcpStream, msg: serde_json::Value) {
        use std::io::Write as _;
        let frame = encode_frame(&serde_json::to_string(&msg).unwrap());
        let mut s = server;
        s.write_all(frame.as_bytes()).unwrap();
    }

    fn server_read(server: &TcpStream) -> serde_json::Value {
        let mut reader = BufReader::new(server);
        recv_from(&mut reader).unwrap()
    }

    #[test]
    fn get_actor_id_parses_device_actor() {
        let (mut transport, server) = make_transport_pair();

        let t = std::thread::spawn(move || {
            let _req = server_read(&server);
            server_reply(
                &server,
                json!({
                    "from": "root",
                    "deviceActor": "server1.conn0.deviceActor1",
                    "screenshotActor": "server1.conn0.screenshotActor7",
                }),
            );
        });

        let id = DeviceActor::get_actor_id(&mut transport).unwrap();
        assert_eq!(id.as_deref(), Some("server1.conn0.deviceActor1"));
        t.join().unwrap();
    }

    #[test]
    fn get_actor_id_returns_none_when_field_absent() {
        let (mut transport, server) = make_transport_pair();

        let t = std::thread::spawn(move || {
            let _req = server_read(&server);
            server_reply(&server, json!({ "from": "root", "screenshotActor": "x" }));
        });

        let id = DeviceActor::get_actor_id(&mut transport).unwrap();
        assert!(id.is_none());
        t.join().unwrap();
    }

    #[test]
    fn query_firefox_version_parses_app_version() {
        let (mut transport, server) = make_transport_pair();

        let t = std::thread::spawn(move || {
            let _req = server_read(&server);
            server_reply(
                &server,
                json!({
                    "from": "server1.conn0.deviceActor1",
                    "value": {
                        "appVersion": "137.0",
                        "platformVersion": "137.0",
                        "appBuildID": "20250101000000",
                    }
                }),
            );
        });

        let version =
            DeviceActor::query_firefox_version(&mut transport, "server1.conn0.deviceActor1")
                .unwrap();
        assert_eq!(version, Some(137));
        t.join().unwrap();
    }

    #[test]
    fn query_firefox_version_handles_alpha_suffix() {
        let (mut transport, server) = make_transport_pair();

        let t = std::thread::spawn(move || {
            let _req = server_read(&server);
            server_reply(
                &server,
                json!({
                    "from": "server1.conn0.deviceActor1",
                    "value": { "appVersion": "138.0a1" }
                }),
            );
        });

        let version =
            DeviceActor::query_firefox_version(&mut transport, "server1.conn0.deviceActor1")
                .unwrap();
        assert_eq!(version, Some(138));
        t.join().unwrap();
    }

    #[test]
    fn query_firefox_version_returns_none_when_app_version_absent() {
        let (mut transport, server) = make_transport_pair();

        let t = std::thread::spawn(move || {
            let _req = server_read(&server);
            server_reply(
                &server,
                json!({
                    "from": "server1.conn0.deviceActor1",
                    "value": { "appBuildID": "20250101" }
                }),
            );
        });

        let version =
            DeviceActor::query_firefox_version(&mut transport, "server1.conn0.deviceActor1")
                .unwrap();
        assert!(version.is_none());
        t.join().unwrap();
    }

    #[test]
    fn parse_app_version_extracts_major() {
        assert_eq!(
            parse_app_version(&json!({"value": {"appVersion": "137.0"}})),
            Some(137)
        );
        assert_eq!(
            parse_app_version(&json!({"value": {"appVersion": "138.0a1"}})),
            Some(138)
        );
        assert_eq!(
            parse_app_version(&json!({"appVersion": "120.0"})),
            Some(120)
        );
    }

    #[test]
    fn parse_app_version_falls_back_to_platform_version() {
        assert_eq!(
            parse_app_version(&json!({"value": {"platformVersion": "135.0"}})),
            Some(135)
        );
    }
}
