use serde_json::Value;

use crate::actor::actor_request;
use crate::actors::tab::TabInfo;
use crate::error::ProtocolError;
use crate::transport::RdpTransport;
use crate::types::ActorId;

/// Metadata for a browser process as returned by `listProcesses`.
#[derive(Debug, Clone)]
pub struct ProcessInfo {
    /// The process descriptor actor ID.
    pub actor: ActorId,
    /// Whether this is the browser parent process.
    pub is_parent: bool,
}

/// Operations on the Firefox RDP root actor (fixed ID `"root"`).
pub struct RootActor;

impl RootActor {
    /// List all open browser tabs.
    ///
    /// Sends `listTabs` to the root actor and parses the response into typed
    /// [`TabInfo`] structs.
    ///
    /// Firefox may interleave `tabListChanged` push events (which carry a
    /// `type` field and no `tabs` field) between our `listTabs` request and
    /// the actual reply.  We skip any packet from `"root"` that has a `type`
    /// field — those are push events, not replies.  The first packet from
    /// `"root"` without `type` is the authoritative listTabs reply.
    pub fn list_tabs(transport: &mut RdpTransport) -> Result<Vec<TabInfo>, ProtocolError> {
        use crate::error::ActorErrorKind;
        use serde_json::json;

        let request = json!({"to": "root", "type": "listTabs"});
        transport.send(&request)?;

        // Read packets until we find the real listTabs reply: from root, no `type`.
        let mut response = loop {
            let msg = transport.recv()?;
            let from = msg.get("from").and_then(Value::as_str).unwrap_or_default();
            if from == "root" {
                if msg.get("type").is_some() {
                    // Push event (e.g. tabListChanged) — skip it.
                    continue;
                }
                // Check for actor-level error response.
                if let Some(error) = msg.get("error").and_then(Value::as_str) {
                    return Err(ProtocolError::ActorError {
                        actor: "root".to_owned(),
                        kind: ActorErrorKind::from_code(error),
                        error: error.to_owned(),
                        message: msg
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_owned(),
                    });
                }
                break msg;
            }
        };

        let tabs_value = response.get_mut("tabs").map(Value::take).ok_or_else(|| {
            ProtocolError::InvalidPacket("listTabs response missing 'tabs' field".into())
        })?;

        let tabs: Vec<TabInfo> = serde_json::from_value(tabs_value)
            .map_err(|e| ProtocolError::InvalidPacket(format!("failed to parse tabs: {e}")))?;

        Ok(tabs)
    }

    /// Get root actor metadata (device, preferences, addons actor IDs).
    pub fn get_root(transport: &mut RdpTransport) -> Result<Value, ProtocolError> {
        actor_request(transport, "root", "getRoot", None)
    }

    /// List all browser processes.
    ///
    /// Returns a list of process descriptors.  Use `is_parent` to find the
    /// browser parent process, which has access to chrome-privileged APIs.
    ///
    /// Available in Firefox 87+.  Returns `Err` on older builds or when the
    /// root actor does not recognise the `listProcesses` request type.
    pub fn list_processes(transport: &mut RdpTransport) -> Result<Vec<ProcessInfo>, ProtocolError> {
        let response = actor_request(transport, "root", "listProcesses", None)?;

        let processes = response
            .get("processes")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                ProtocolError::InvalidPacket(
                    "listProcesses response missing 'processes' field".into(),
                )
            })?;

        // Parse each entry explicitly so malformed entries fail fast instead
        // of being silently dropped (CodeRabbit review feedback on PR #73).
        let result: Result<Vec<ProcessInfo>, ProtocolError> = processes
            .iter()
            .enumerate()
            .map(|(idx, p)| {
                let actor_str = p.get("actor").and_then(Value::as_str).ok_or_else(|| {
                    ProtocolError::InvalidPacket(format!(
                        "listProcesses entry {idx} missing or non-string 'actor' field: {p}"
                    ))
                })?;
                // `isParent` is documented as optional in older Firefox builds;
                // default to `false` when absent, but reject non-bool values.
                let is_parent = match p.get("isParent") {
                    None | Some(Value::Null) => false,
                    Some(Value::Bool(b)) => *b,
                    Some(other) => {
                        return Err(ProtocolError::InvalidPacket(format!(
                            "listProcesses entry {idx} has non-bool 'isParent': {other}"
                        )));
                    }
                };
                Ok(ProcessInfo {
                    actor: ActorId::from(actor_str),
                    is_parent,
                })
            })
            .collect();

        result
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufReader, Write};
    use std::net::{TcpListener, TcpStream};

    use serde_json::json;

    use super::*;
    use crate::transport::{RdpTransport, encode_frame, recv_from};

    /// Spin up a minimal in-process server that sends `response` back to the
    /// first request it receives.
    fn make_transport_with_response(response: serde_json::Value) -> RdpTransport {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let client = TcpStream::connect(addr).unwrap();
        let (accept, _) = listener.accept().unwrap();

        std::thread::spawn(move || {
            let mut srv_reader = BufReader::new(&accept);
            // Consume the single request, then reply.
            let _ = recv_from(&mut srv_reader).unwrap();
            let frame = encode_frame(&serde_json::to_string(&response).unwrap());
            (&accept).write_all(frame.as_bytes()).unwrap();
        });

        let writer = client.try_clone().unwrap();
        let reader = BufReader::new(client);
        RdpTransport::from_parts(reader, writer)
    }

    #[test]
    fn list_processes_happy_path_two_processes() {
        let response = json!({
            "from": "root",
            "processes": [
                { "actor": "server1.conn0.processDescriptor1", "isParent": true },
                { "actor": "server1.conn0.processDescriptor2", "isParent": false }
            ]
        });
        let mut transport = make_transport_with_response(response);
        let procs = RootActor::list_processes(&mut transport).unwrap();
        assert_eq!(procs.len(), 2);
        assert_eq!(procs[0].actor.as_ref(), "server1.conn0.processDescriptor1");
        assert!(procs[0].is_parent);
        assert_eq!(procs[1].actor.as_ref(), "server1.conn0.processDescriptor2");
        assert!(!procs[1].is_parent);
    }

    #[test]
    fn list_processes_missing_processes_field_returns_error() {
        let response = json!({ "from": "root" });
        let mut transport = make_transport_with_response(response);
        let err = RootActor::list_processes(&mut transport).unwrap_err();
        assert!(
            err.to_string().contains("'processes'"),
            "error should mention 'processes': {err}"
        );
        assert!(
            matches!(err, ProtocolError::InvalidPacket(_)),
            "expected InvalidPacket, got {err:?}"
        );
    }

    #[test]
    fn list_processes_entry_missing_actor_fails_fast() {
        // An entry without an `actor` field must now fail with a clear error
        // (CodeRabbit PR #73 feedback: don't silently drop malformed entries).
        let response = json!({
            "from": "root",
            "processes": [
                { "isParent": true },
                { "actor": "server1.conn0.processDescriptor2", "isParent": false }
            ]
        });
        let mut transport = make_transport_with_response(response);
        let err = RootActor::list_processes(&mut transport).unwrap_err();
        assert!(
            matches!(err, ProtocolError::InvalidPacket(_)),
            "expected InvalidPacket, got {err:?}"
        );
        assert!(
            err.to_string().contains("'actor'"),
            "error should mention the missing 'actor' field: {err}"
        );
    }

    #[test]
    fn list_processes_entry_non_bool_is_parent_fails_fast() {
        let response = json!({
            "from": "root",
            "processes": [
                { "actor": "server1.conn0.processDescriptor1", "isParent": "yes" }
            ]
        });
        let mut transport = make_transport_with_response(response);
        let err = RootActor::list_processes(&mut transport).unwrap_err();
        assert!(
            matches!(err, ProtocolError::InvalidPacket(_)),
            "expected InvalidPacket, got {err:?}"
        );
        assert!(
            err.to_string().contains("'isParent'"),
            "error should mention 'isParent': {err}"
        );
    }

    #[test]
    fn list_processes_missing_is_parent_defaults_to_false() {
        // `isParent` is documented as optional on some Firefox builds.
        let response = json!({
            "from": "root",
            "processes": [
                { "actor": "server1.conn0.processDescriptor1" }
            ]
        });
        let mut transport = make_transport_with_response(response);
        let procs = RootActor::list_processes(&mut transport).unwrap();
        assert_eq!(procs.len(), 1);
        assert!(!procs[0].is_parent);
    }
}
