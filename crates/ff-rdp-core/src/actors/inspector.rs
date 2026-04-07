use serde_json::Value;

use crate::actor::actor_request;
use crate::error::ProtocolError;
use crate::transport::RdpTransport;
use crate::types::ActorId;

/// Operations on the Firefox InspectorActor.
pub struct InspectorActor;

impl InspectorActor {
    /// Call `getWalker` on the inspector actor to get the DOM walker actor ID.
    ///
    /// Response: `{"walker": {"actor": "server1.conn0.child0/domwalkerActor1", ...}, ...}`
    pub fn get_walker(
        transport: &mut RdpTransport,
        inspector_actor: &ActorId,
    ) -> Result<ActorId, ProtocolError> {
        let response = actor_request(transport, inspector_actor.as_ref(), "getWalker", None)?;

        // Response: {"walker": {"actor": "..."}, ...}
        let walker_actor = response
            .get("walker")
            .and_then(|w| w.get("actor"))
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ProtocolError::InvalidPacket(
                    "getWalker response missing 'walker.actor' field".into(),
                )
            })?;

        Ok(walker_actor.into())
    }

    /// Call `getPageStyle` on the inspector actor to get the page style actor ID.
    ///
    /// Response: `{"pageStyle": {"actor": "server1.conn0.child0/pageStyleActor1", ...}, ...}`
    pub fn get_page_style(
        transport: &mut RdpTransport,
        inspector_actor: &ActorId,
    ) -> Result<ActorId, ProtocolError> {
        let response = actor_request(transport, inspector_actor.as_ref(), "getPageStyle", None)?;

        // Response: {"pageStyle": {"actor": "..."}, ...}
        let page_style_actor = response
            .get("pageStyle")
            .and_then(|p| p.get("actor"))
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ProtocolError::InvalidPacket(
                    "getPageStyle response missing 'pageStyle.actor' field".into(),
                )
            })?;

        Ok(page_style_actor.into())
    }
}

#[cfg(test)]
mod tests {
    /// Helper to simulate parsing get_walker-style response JSON.
    fn extract_walker_actor(response: &serde_json::Value) -> Option<&str> {
        response
            .get("walker")
            .and_then(|w| w.get("actor"))
            .and_then(serde_json::Value::as_str)
    }

    /// Helper to simulate parsing get_page_style-style response JSON.
    fn extract_page_style_actor(response: &serde_json::Value) -> Option<&str> {
        response
            .get("pageStyle")
            .and_then(|p| p.get("actor"))
            .and_then(serde_json::Value::as_str)
    }

    #[test]
    fn parse_get_walker_response_extracts_actor() {
        let response = serde_json::json!({
            "walker": {
                "actor": "server1.conn0.child0/domwalkerActor1",
                "rootNode": {}
            },
            "from": "server1.conn0.child0/inspectorActor1"
        });
        let actor = extract_walker_actor(&response).unwrap();
        assert_eq!(actor, "server1.conn0.child0/domwalkerActor1");
    }

    #[test]
    fn parse_get_walker_response_missing_walker_returns_none() {
        let response = serde_json::json!({"from": "server1.conn0.child0/inspectorActor1"});
        assert!(extract_walker_actor(&response).is_none());
    }

    #[test]
    fn parse_get_walker_response_missing_actor_inside_walker_returns_none() {
        let response = serde_json::json!({
            "walker": {"rootNode": {}},
            "from": "server1.conn0.child0/inspectorActor1"
        });
        assert!(extract_walker_actor(&response).is_none());
    }

    #[test]
    fn parse_get_page_style_response_extracts_actor() {
        let response = serde_json::json!({
            "pageStyle": {
                "actor": "server1.conn0.child0/pageStyleActor1"
            },
            "from": "server1.conn0.child0/inspectorActor1"
        });
        let actor = extract_page_style_actor(&response).unwrap();
        assert_eq!(actor, "server1.conn0.child0/pageStyleActor1");
    }

    #[test]
    fn parse_get_page_style_response_missing_page_style_returns_none() {
        let response = serde_json::json!({"from": "server1.conn0.child0/inspectorActor1"});
        assert!(extract_page_style_actor(&response).is_none());
    }

    #[test]
    fn parse_get_page_style_response_missing_actor_inside_returns_none() {
        let response = serde_json::json!({
            "pageStyle": {},
            "from": "server1.conn0.child0/inspectorActor1"
        });
        assert!(extract_page_style_actor(&response).is_none());
    }
}
