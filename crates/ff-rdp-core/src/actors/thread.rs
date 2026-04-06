use serde_json::Value;

use crate::actor::actor_request;
use crate::error::ProtocolError;
use crate::transport::RdpTransport;

/// Information about a loaded JavaScript/WASM source.
#[derive(Debug, Clone)]
pub struct SourceInfo {
    /// The source actor ID.
    pub actor: String,
    /// URL of the source (may be empty for eval'd code).
    pub url: String,
    /// Whether the source is black-boxed (skipped during debugging).
    pub is_black_boxed: bool,
}

/// Operations on a ThreadActor (source listing, attach/resume/detach lifecycle).
///
/// Thread state machine:
///   Detached → attach → Paused → resume → Running → detach
///
/// IMPORTANT: After attaching and reading sources, always resume before
/// detaching. Skipping resume leaves the page frozen.
pub struct ThreadActor;

impl ThreadActor {
    /// Attach to the thread actor, transitioning it from Detached to Paused.
    pub fn attach(
        transport: &mut RdpTransport,
        thread_actor: &str,
    ) -> Result<Value, ProtocolError> {
        actor_request(transport, thread_actor, "attach", None)
    }

    /// List all sources loaded in the thread.
    ///
    /// The thread must be in the Paused state (call [`Self::attach`] first).
    pub fn sources(
        transport: &mut RdpTransport,
        thread_actor: &str,
    ) -> Result<Vec<SourceInfo>, ProtocolError> {
        let response = actor_request(transport, thread_actor, "sources", None)?;
        let sources = response
            .get("sources")
            .and_then(Value::as_array)
            .map(|arr| arr.iter().filter_map(parse_source_info).collect())
            .unwrap_or_default();
        Ok(sources)
    }

    /// Resume the thread, transitioning from Paused to Running.
    ///
    /// MUST be called after [`Self::attach`] + [`Self::sources`] to avoid
    /// freezing the page.
    pub fn resume(
        transport: &mut RdpTransport,
        thread_actor: &str,
    ) -> Result<Value, ProtocolError> {
        actor_request(transport, thread_actor, "resume", None)
    }

    /// Detach from the thread, transitioning to Detached.
    pub fn detach(
        transport: &mut RdpTransport,
        thread_actor: &str,
    ) -> Result<Value, ProtocolError> {
        actor_request(transport, thread_actor, "detach", None)
    }

    /// Convenience: attach, list sources, resume, detach — with cleanup on error.
    ///
    /// Ensures resume + detach are called even when `sources` fails, so the
    /// page is never left in a frozen Paused state.
    pub fn list_sources(
        transport: &mut RdpTransport,
        thread_actor: &str,
    ) -> Result<Vec<SourceInfo>, ProtocolError> {
        Self::attach(transport, thread_actor)?;

        let sources = match Self::sources(transport, thread_actor) {
            Ok(s) => s,
            Err(e) => {
                // Best-effort cleanup: resume then detach to avoid leaving the
                // page frozen. Errors from cleanup are intentionally discarded.
                let _ = Self::resume(transport, thread_actor);
                let _ = Self::detach(transport, thread_actor);
                return Err(e);
            }
        };

        // Resume first (Paused → Running), then detach.
        Self::resume(transport, thread_actor)?;
        Self::detach(transport, thread_actor)?;

        Ok(sources)
    }
}

/// Parse a single source entry from the `sources` response array.
fn parse_source_info(value: &Value) -> Option<SourceInfo> {
    let actor = value.get("actor")?.as_str()?.to_owned();
    let url = value
        .get("url")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let is_black_boxed = value
        .get("isBlackBoxed")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    Some(SourceInfo {
        actor,
        url,
        is_black_boxed,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn parse_source_info_valid() {
        let value = json!({
            "actor": "server1.conn0.child1/sourceActor42",
            "url": "https://example.com/app.js",
            "isBlackBoxed": false
        });
        let info = parse_source_info(&value).unwrap();
        assert_eq!(info.actor, "server1.conn0.child1/sourceActor42");
        assert_eq!(info.url, "https://example.com/app.js");
        assert!(!info.is_black_boxed);
    }

    #[test]
    fn parse_source_info_black_boxed() {
        let value = json!({
            "actor": "server1.conn0.child1/sourceActor10",
            "url": "https://example.com/vendor.min.js",
            "isBlackBoxed": true
        });
        let info = parse_source_info(&value).unwrap();
        assert!(info.is_black_boxed);
    }

    #[test]
    fn parse_source_info_missing_url_defaults_to_empty() {
        // Eval'd code may have no URL field at all.
        let value = json!({
            "actor": "server1.conn0.child1/sourceActor99"
        });
        let info = parse_source_info(&value).unwrap();
        assert_eq!(info.url, "");
        assert!(!info.is_black_boxed);
    }

    #[test]
    fn parse_source_info_null_url_defaults_to_empty() {
        let value = json!({
            "actor": "server1.conn0.child1/sourceActor99",
            "url": null
        });
        let info = parse_source_info(&value).unwrap();
        assert_eq!(info.url, "");
    }

    #[test]
    fn parse_source_info_missing_actor_returns_none() {
        // actor is required; without it we cannot address the source.
        let value = json!({
            "url": "https://example.com/app.js",
            "isBlackBoxed": false
        });
        assert!(parse_source_info(&value).is_none());
    }

    #[test]
    fn parse_source_info_missing_is_black_boxed_defaults_to_false() {
        let value = json!({
            "actor": "server1.conn0.child1/sourceActor1",
            "url": "https://example.com/main.js"
        });
        let info = parse_source_info(&value).unwrap();
        assert!(!info.is_black_boxed);
    }

    #[test]
    fn sources_response_empty_array() {
        // Verify that a "sources": [] response yields an empty Vec without error.
        // We use parse_source_info directly since wiring up a full mock transport
        // is done in actor.rs tests.
        let empty: Vec<SourceInfo> = json!([])
            .as_array()
            .unwrap()
            .iter()
            .filter_map(parse_source_info)
            .collect();
        assert!(empty.is_empty());
    }

    #[test]
    fn sources_response_multiple() {
        let arr = json!([
            {"actor": "server1.conn0.child1/src1", "url": "https://a.com/a.js", "isBlackBoxed": false},
            {"actor": "server1.conn0.child1/src2", "url": "https://a.com/b.js", "isBlackBoxed": true},
            {"actor": "server1.conn0.child1/src3", "url": "",                   "isBlackBoxed": false},
        ]);
        let infos: Vec<SourceInfo> = arr
            .as_array()
            .unwrap()
            .iter()
            .filter_map(parse_source_info)
            .collect();
        assert_eq!(infos.len(), 3);
        assert_eq!(infos[1].actor, "server1.conn0.child1/src2");
        assert!(infos[1].is_black_boxed);
        assert_eq!(infos[2].url, "");
    }
}
