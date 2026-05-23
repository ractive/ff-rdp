//! Spec for the WebConsole actor.
//!
//! Mirrors <https://searchfox.org/mozilla-central/source/devtools/shared/specs/webconsole.js>
//!
//! Note: `evaluateJSAsync` uses a two-packet exchange (immediate ack → async event) that
//! cannot be handled by the generic [`super::call`] helper.  The Front method delegates to
//! the existing `actors::console::WebConsoleActor::evaluate_js_async` implementation which
//! handles that event loop.  The args struct is still typed here so the call site is Value-free.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Method, sealed};

// ---------------------------------------------------------------------------
// Request args
// ---------------------------------------------------------------------------

pub mod request {
    use super::Serialize;

    /// Args for `startListeners`.
    #[derive(Debug, Clone, Default, Serialize)]
    pub struct StartListeners {
        pub listeners: Vec<String>,
    }

    /// Args for `stopListeners`.
    #[derive(Debug, Clone, Default, Serialize)]
    pub struct StopListeners {
        pub listeners: Vec<String>,
    }

    /// Args for `getCachedMessages`.
    #[derive(Debug, Clone, Default, Serialize)]
    pub struct GetCachedMessages {
        #[serde(rename = "messageTypes")]
        pub message_types: Vec<String>,
    }

    /// Args for `evaluateJSAsync` — typed so Front callers are Value-free.
    ///
    /// Note: the two-packet protocol is handled in the Front method, not by the
    /// generic `call` helper.
    ///
    /// Chrome-privileged evaluation goes through the parent-process console
    /// actor obtained via `root.getProcess(0)` → `processDescriptor.getTarget()`.
    /// The `chromeContext` field is intentionally absent from this struct: the
    /// parent-process console actor is chrome-privileged by design.
    #[derive(Debug, Clone, Serialize)]
    pub struct EvaluateJsAsync {
        pub text: String,
        #[serde(default)]
        pub eager: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub mapped: Option<EvaluateMapped>,
    }

    /// The `mapped` field for `evaluateJSAsync`.
    #[derive(Debug, Clone, Default, Serialize)]
    pub struct EvaluateMapped {
        #[serde(rename = "await")]
        pub await_promise: bool,
    }
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

pub mod response {
    use super::{Deserialize, Value};

    /// Reply for `startListeners`.
    ///
    /// Firefox returns the confirmed listener names in the `startedListeners` key
    /// (not `listeners`).  See `devtools/shared/specs/webconsole.js`.
    #[derive(Debug, Clone, Default, Deserialize)]
    pub struct StartListeners {
        /// Listeners successfully started — wire key is `startedListeners`.
        #[serde(rename = "startedListeners", default)]
        pub listeners: Vec<String>,
    }

    /// Reply for `stopListeners`.
    ///
    /// Firefox returns the confirmed listener names in the `stoppedListeners` key.
    #[derive(Debug, Clone, Default, Deserialize)]
    pub struct StopListeners {
        /// Listeners successfully stopped — wire key is `stoppedListeners`.
        #[serde(rename = "stoppedListeners", default)]
        pub listeners: Vec<String>,
    }

    /// Reply for `getCachedMessages`.
    #[derive(Debug, Clone, Default, Deserialize)]
    pub struct GetCachedMessages {
        #[serde(default)]
        pub messages: Vec<Value>,
    }
}

// ---------------------------------------------------------------------------
// Method markers
// ---------------------------------------------------------------------------

/// `startListeners` method marker.
pub struct StartListeners;
impl sealed::Sealed for StartListeners {}
impl Method for StartListeners {
    const NAME: &'static str = "startListeners";
    type Args = request::StartListeners;
    type Reply = response::StartListeners;
}

/// `stopListeners` method marker.
pub struct StopListeners;
impl sealed::Sealed for StopListeners {}
impl Method for StopListeners {
    const NAME: &'static str = "stopListeners";
    type Args = request::StopListeners;
    type Reply = response::StopListeners;
}

/// `getCachedMessages` method marker.
pub struct GetCachedMessages;
impl sealed::Sealed for GetCachedMessages {}
impl Method for GetCachedMessages {
    const NAME: &'static str = "getCachedMessages";
    type Args = request::GetCachedMessages;
    type Reply = response::GetCachedMessages;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn start_listeners_request_serializes_listeners() {
        let args = request::StartListeners {
            listeners: vec!["PageError".into(), "ConsoleAPI".into()],
        };
        let v = serde_json::to_value(&args).unwrap();
        assert_eq!(v["listeners"], json!(["PageError", "ConsoleAPI"]));
    }

    #[test]
    fn stop_listeners_request_serializes_listeners() {
        let args = request::StopListeners {
            listeners: vec!["PageError".into()],
        };
        let v = serde_json::to_value(&args).unwrap();
        assert_eq!(v["listeners"], json!(["PageError"]));
    }

    #[test]
    fn get_cached_messages_request_uses_camel_case_key() {
        let args = request::GetCachedMessages {
            message_types: vec!["PageError".into(), "ConsoleAPI".into()],
        };
        let v = serde_json::to_value(&args).unwrap();
        assert_eq!(v["messageTypes"], json!(["PageError", "ConsoleAPI"]));
        assert!(
            v.get("message_types").is_none(),
            "must not use snake_case key"
        );
    }

    #[test]
    fn start_listeners_response_deserializes_started_listeners_key() {
        // Firefox returns `startedListeners`, not `listeners`.
        // The old key name (`listeners`) should produce an empty Vec via #[serde(default)].
        let v =
            json!({"from": "conn0/consoleActor1", "startedListeners": ["PageError", "ConsoleAPI"]});
        let reply: response::StartListeners = serde_json::from_value(v).unwrap();
        assert_eq!(reply.listeners, ["PageError", "ConsoleAPI"]);
    }

    #[test]
    fn start_listeners_response_old_key_gives_empty_vec() {
        // If Firefox ever returns the old `listeners` key we should NOT panic;
        // we silently get an empty Vec and the caller can decide.
        let v = json!({"from": "conn0/consoleActor1", "listeners": ["PageError"]});
        let reply: response::StartListeners = serde_json::from_value(v).unwrap();
        assert!(reply.listeners.is_empty());
    }

    #[test]
    fn stop_listeners_response_deserializes_stopped_listeners_key() {
        let v = json!({"from": "conn0/consoleActor1", "stoppedListeners": ["PageError"]});
        let reply: response::StopListeners = serde_json::from_value(v).unwrap();
        assert_eq!(reply.listeners, ["PageError"]);
    }

    #[test]
    fn get_cached_messages_response_deserializes_messages() {
        let v = json!({"from": "conn0/consoleActor1", "messages": [{"level": "log"}]});
        let reply: response::GetCachedMessages = serde_json::from_value(v).unwrap();
        assert_eq!(reply.messages.len(), 1);
    }

    #[test]
    fn method_names_are_correct() {
        assert_eq!(StartListeners::NAME, "startListeners");
        assert_eq!(StopListeners::NAME, "stopListeners");
        assert_eq!(GetCachedMessages::NAME, "getCachedMessages");
    }
}
