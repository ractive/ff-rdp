/// Resource types that the Watcher actor can subscribe to.
///
/// Each variant corresponds to a Firefox DevTools resource type string.
/// See the Firefox DevTools `ResourceCommand` source for the full list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResourceType {
    /// HTTP network request/response events (`"network-event"`).
    NetworkEvent,
    /// Console log/warn/error/info messages (`"console-message"`).
    ConsoleMessage,
    /// JS exceptions and page errors (`"error-message"`).
    ErrorMessage,
    /// Document lifecycle events: DOMContentLoaded, load, navigate (`"document-event"`).
    DocumentEvent,
    /// CSS stylesheet changes (`"css-change"`).
    CssChange,
    /// Thread pause/resume state for debugger (`"thread-state"`).
    ThreadState,
}

impl ResourceType {
    /// Return the wire-format string used in `watchResources` / `unwatchResources` calls.
    pub fn as_wire_str(self) -> &'static str {
        match self {
            Self::NetworkEvent => "network-event",
            Self::ConsoleMessage => "console-message",
            Self::ErrorMessage => "error-message",
            Self::DocumentEvent => "document-event",
            Self::CssChange => "css-change",
            Self::ThreadState => "thread-state",
        }
    }

    /// Parse a wire-format string into a `ResourceType`, returning `None` for unknown types.
    pub fn from_wire_str(s: &str) -> Option<Self> {
        match s {
            "network-event" => Some(Self::NetworkEvent),
            "console-message" => Some(Self::ConsoleMessage),
            "error-message" => Some(Self::ErrorMessage),
            "document-event" => Some(Self::DocumentEvent),
            "css-change" => Some(Self::CssChange),
            "thread-state" => Some(Self::ThreadState),
            _ => None,
        }
    }
}

impl std::fmt::Display for ResourceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_wire_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_wire_strings() {
        let types = [
            ResourceType::NetworkEvent,
            ResourceType::ConsoleMessage,
            ResourceType::ErrorMessage,
            ResourceType::DocumentEvent,
            ResourceType::CssChange,
            ResourceType::ThreadState,
        ];
        for rt in types {
            let wire = rt.as_wire_str();
            let parsed = ResourceType::from_wire_str(wire)
                .unwrap_or_else(|| panic!("from_wire_str({wire:?}) should succeed for {rt:?}"));
            assert_eq!(rt, parsed, "round-trip failed for {rt:?}");
        }
    }

    #[test]
    fn unknown_wire_str_returns_none() {
        assert!(ResourceType::from_wire_str("unknown-type").is_none());
        assert!(ResourceType::from_wire_str("").is_none());
    }

    #[test]
    fn display_matches_wire_str() {
        assert_eq!(ResourceType::NetworkEvent.to_string(), "network-event");
        assert_eq!(ResourceType::ConsoleMessage.to_string(), "console-message");
    }
}
