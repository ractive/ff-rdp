use serde_json::{Value, json};

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
        use crate::actor::actor_request;
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
        let request = json!({"to": "root", "type": "listProcesses"});
        transport.send(&request)?;

        let response = loop {
            let msg = transport.recv()?;
            let from = msg.get("from").and_then(Value::as_str).unwrap_or_default();
            if from == "root" {
                if msg.get("type").is_some() {
                    continue;
                }
                if let Some(error) = msg.get("error").and_then(Value::as_str) {
                    return Err(ProtocolError::ActorError {
                        actor: "root".to_owned(),
                        kind: crate::error::ActorErrorKind::from_code(error),
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

        let processes = response
            .get("processes")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                ProtocolError::InvalidPacket(
                    "listProcesses response missing 'processes' field".into(),
                )
            })?;

        let result = processes
            .iter()
            .filter_map(|p| {
                let actor_str = p.get("actor").and_then(Value::as_str)?;
                let is_parent = p.get("isParent").and_then(Value::as_bool).unwrap_or(false);
                Some(ProcessInfo {
                    actor: ActorId::from(actor_str),
                    is_parent,
                })
            })
            .collect();

        Ok(result)
    }
}
