//! Spec for the WindowGlobalTarget actor.
//!
//! Mirrors <https://searchfox.org/mozilla-central/source/devtools/shared/specs/targets/window-global.js>

use serde::{Deserialize, Serialize};

use super::{Method, NoArgs, sealed};

// ---------------------------------------------------------------------------
// Request args
// ---------------------------------------------------------------------------

pub mod request {
    use super::{NoArgs, Serialize};

    /// Args for `navigateTo`.
    #[derive(Debug, Clone, Serialize)]
    pub struct NavigateTo {
        pub url: String,
    }

    /// Args for `reload` — no parameters.
    pub type Reload = NoArgs;

    /// Args for `goBack` — no parameters.
    pub type GoBack = NoArgs;

    /// Args for `goForward` — no parameters.
    pub type GoForward = NoArgs;
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

pub mod response {
    use super::Deserialize;

    /// Reply for `navigateTo` — empty acknowledgement.
    #[derive(Debug, Clone, Default, Deserialize)]
    pub struct NavigateTo {}

    /// Reply for `reload` — empty acknowledgement.
    #[derive(Debug, Clone, Default, Deserialize)]
    pub struct Reload {}

    /// Reply for `goBack` — empty acknowledgement.
    #[derive(Debug, Clone, Default, Deserialize)]
    pub struct GoBack {}

    /// Reply for `goForward` — empty acknowledgement.
    #[derive(Debug, Clone, Default, Deserialize)]
    pub struct GoForward {}
}

// ---------------------------------------------------------------------------
// Method markers
// ---------------------------------------------------------------------------

/// `navigateTo` method marker.
pub struct NavigateTo;
impl sealed::Sealed for NavigateTo {}
impl Method for NavigateTo {
    const NAME: &'static str = "navigateTo";
    type Args = request::NavigateTo;
    type Reply = response::NavigateTo;
}

/// `reload` method marker.
pub struct Reload;
impl sealed::Sealed for Reload {}
impl Method for Reload {
    const NAME: &'static str = "reload";
    type Args = NoArgs;
    type Reply = response::Reload;
}

/// `goBack` method marker.
pub struct GoBack;
impl sealed::Sealed for GoBack {}
impl Method for GoBack {
    const NAME: &'static str = "goBack";
    type Args = NoArgs;
    type Reply = response::GoBack;
}

/// `goForward` method marker.
pub struct GoForward;
impl sealed::Sealed for GoForward {}
impl Method for GoForward {
    const NAME: &'static str = "goForward";
    type Args = NoArgs;
    type Reply = response::GoForward;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn navigate_to_request_serializes_url() {
        let args = request::NavigateTo {
            url: "https://example.com".into(),
        };
        let v = serde_json::to_value(&args).unwrap();
        assert_eq!(v["url"], "https://example.com");
    }

    #[test]
    fn reload_response_deserializes_empty() {
        let v = json!({"from": "server1.conn0.child1/windowGlobalTarget1"});
        let _: response::Reload = serde_json::from_value(v).unwrap();
    }

    #[test]
    fn navigate_to_response_deserializes_empty() {
        let v = json!({"from": "server1.conn0.child1/windowGlobalTarget1"});
        let _: response::NavigateTo = serde_json::from_value(v).unwrap();
    }

    #[test]
    fn method_names_are_correct() {
        assert_eq!(NavigateTo::NAME, "navigateTo");
        assert_eq!(Reload::NAME, "reload");
        assert_eq!(GoBack::NAME, "goBack");
        assert_eq!(GoForward::NAME, "goForward");
    }
}
