use serde_json::Value;

use crate::actors::watcher::{ConsoleResource, NetworkResource, NetworkResourceUpdate};

/// A typed resource event received from the Firefox Watcher actor.
///
/// Each variant corresponds to a [`super::ResourceType`] and carries a typed
/// payload extracted from the wire JSON. The "don't over-model" rule applies:
/// variants only carry fields that consumers actually need on day one.
#[derive(Debug, Clone)]
pub enum Resource {
    /// A network request/response pair (`"network-event"`).
    NetworkEvent(NetworkResource),

    /// An update to an existing network event (status, headers, timing).
    NetworkUpdate(NetworkResourceUpdate),

    /// A console message (`"console-message"`).
    ConsoleMessage(ConsoleResource),

    /// A JS exception or page error (`"error-message"`).
    ErrorMessage(ConsoleResource),

    /// A raw document lifecycle event (`"document-event"`).
    ///
    /// Delivered as raw JSON until a typed `DocumentEvent` struct is needed.
    DocumentEvent(Value),
}

impl Resource {
    /// Return the wire-format type name for this resource.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::NetworkEvent(_) | Self::NetworkUpdate(_) => "network-event",
            Self::ConsoleMessage(_) => "console-message",
            Self::ErrorMessage(_) => "error-message",
            Self::DocumentEvent(_) => "document-event",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ActorId;

    fn dummy_network_resource() -> NetworkResource {
        NetworkResource {
            actor: ActorId::from("conn0/netEvent1"),
            method: "GET".into(),
            url: "https://example.com/".into(),
            is_xhr: false,
            cause_type: "document".into(),
            started_date_time: "2026-01-01T00:00:00Z".into(),
            timestamp: 0.0,
            resource_id: 1,
        }
    }

    fn dummy_console_resource() -> ConsoleResource {
        ConsoleResource {
            level: "log".into(),
            message: "hello".into(),
            source: "test.js".into(),
            line: 1,
            column: 0,
            timestamp: 0.0,
            resource_id: None,
        }
    }

    #[test]
    fn type_name_matches_wire_format() {
        assert_eq!(
            Resource::NetworkEvent(dummy_network_resource()).type_name(),
            "network-event"
        );
        assert_eq!(
            Resource::NetworkUpdate(NetworkResourceUpdate {
                resource_id: 1,
                ..Default::default()
            })
            .type_name(),
            "network-event"
        );
        assert_eq!(
            Resource::ConsoleMessage(dummy_console_resource()).type_name(),
            "console-message"
        );
        assert_eq!(
            Resource::ErrorMessage(dummy_console_resource()).type_name(),
            "error-message"
        );
        assert_eq!(
            Resource::DocumentEvent(serde_json::json!({"type": "dom-complete"})).type_name(),
            "document-event"
        );
    }
}
