use serde_json::Value;

use crate::actor::actor_request;
use crate::actors::tab::TabInfo;
use crate::error::ProtocolError;
use crate::transport::RdpTransport;

/// How long to wait before retrying a `listTabs` request that returned an
/// incomplete response (no `tabs` field).  This can happen transiently in
/// daemon mode immediately after a navigation, when the Firefox devtools
/// protocol is still processing the page-unload internally.
const LIST_TABS_RETRY_DELAY_MS: u64 = 150;

/// Operations on the Firefox RDP root actor (fixed ID `"root"`).
pub struct RootActor;

impl RootActor {
    /// List all open browser tabs.
    ///
    /// Sends `listTabs` to the root actor and parses the response into typed
    /// [`TabInfo`] structs.
    ///
    /// A single retry with a short delay is attempted if the first response is
    /// missing the `tabs` field.  This guards against a transient race condition
    /// observed in daemon mode immediately after a `navigate` command, where
    /// Firefox occasionally returns an incomplete packet before the devtools
    /// protocol has finished processing the navigation internally.
    pub fn list_tabs(transport: &mut RdpTransport) -> Result<Vec<TabInfo>, ProtocolError> {
        let mut response = actor_request(transport, "root", "listTabs", None)?;

        // Guard against a transient race where Firefox returns a response without
        // the `tabs` field (observed after rapid navigate → eval sequences in daemon
        // mode).  Retry once after a short pause before surfacing the error.
        if response.get("tabs").is_none() {
            std::thread::sleep(std::time::Duration::from_millis(LIST_TABS_RETRY_DELAY_MS));
            response = actor_request(transport, "root", "listTabs", None)?;
        }

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
