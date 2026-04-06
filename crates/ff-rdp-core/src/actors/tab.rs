use serde::{Deserialize, Serialize};

use crate::types::ActorId;

/// Metadata for a browser tab as returned by the root actor's `listTabs`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TabInfo {
    /// The tab descriptor actor ID.
    #[serde(deserialize_with = "deserialize_actor_id")]
    pub actor: ActorId,
    /// Page title.
    #[serde(default)]
    pub title: String,
    /// Current URL.
    #[serde(default)]
    pub url: String,
    /// Whether this tab is currently selected/active.
    #[serde(default)]
    pub selected: bool,
    /// Browsing context identifier (may be absent in older Firefox versions).
    ///
    /// Firefox sends this as `browsingContextID` (uppercase D), which does not
    /// match the `camelCase` rename of this field name, so we override it.
    #[serde(default, rename = "browsingContextID")]
    pub browsing_context_id: Option<u64>,
}

fn deserialize_actor_id<'de, D>(deserializer: D) -> Result<ActorId, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    Ok(ActorId::from(s))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn tab_info_deserializes_from_firefox_response() {
        let v = json!({
            "actor": "server1.conn0.tabDescriptor1",
            "title": "Example",
            "url": "https://example.com",
            "selected": true,
            "browsingContextID": 42
        });
        let tab: TabInfo = serde_json::from_value(v).unwrap();
        assert_eq!(tab.actor.as_ref(), "server1.conn0.tabDescriptor1");
        assert_eq!(tab.title, "Example");
        assert_eq!(tab.url, "https://example.com");
        assert!(tab.selected);
        assert_eq!(tab.browsing_context_id, Some(42));
    }

    #[test]
    fn tab_info_handles_missing_optional_fields() {
        let v = json!({
            "actor": "server1.conn0.tabDescriptor1"
        });
        let tab: TabInfo = serde_json::from_value(v).unwrap();
        assert_eq!(tab.title, "");
        assert_eq!(tab.url, "");
        assert!(!tab.selected);
        assert_eq!(tab.browsing_context_id, None);
    }

    #[test]
    fn tab_info_serializes_to_json() {
        let tab = TabInfo {
            actor: ActorId::from("tab1"),
            title: "Test".into(),
            url: "https://test.com".into(),
            selected: false,
            browsing_context_id: Some(1),
        };
        let v = serde_json::to_value(&tab).unwrap();
        assert_eq!(v["actor"], "tab1");
        assert_eq!(v["browsingContextID"], 1);
    }
}
