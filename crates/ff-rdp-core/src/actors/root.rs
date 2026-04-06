use serde_json::Value;

use crate::actor::actor_request;
use crate::actors::tab::TabInfo;
use crate::error::ProtocolError;
use crate::transport::RdpTransport;

/// Operations on the Firefox RDP root actor (fixed ID `"root"`).
pub struct RootActor;

impl RootActor {
    /// List all open browser tabs.
    ///
    /// Sends `listTabs` to the root actor and parses the response into typed
    /// [`TabInfo`] structs.
    pub fn list_tabs(transport: &mut RdpTransport) -> Result<Vec<TabInfo>, ProtocolError> {
        let mut response = actor_request(transport, "root", "listTabs", None)?;

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
}
