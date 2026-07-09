//! Spec for the Manifest actor (Web App Manifest fetch + validation).
//!
//! Mirrors <https://searchfox.org/mozilla-central/source/devtools/shared/specs/manifest.js>
//!
//! The `manifestActor` ID is exposed on the target frame returned by
//! `getTarget` (see [`crate::actors::tab::TargetInfo::manifest_actor`]); Firefox
//! creates it lazily on first access.
//!
//! `fetchCanonicalManifest` performs the WHATWG "obtain a manifest" steps and
//! returns the parsed manifest together with conformance errors in a single
//! call — a PWA-readiness audit primitive.  We keep the reply as a raw
//! [`serde_json::Value`] so the exact (version-dependent) payload shape is
//! projected at the call site rather than pinned in the type, tolerating
//! unknown keys.

use serde::Deserialize;
use serde_json::Value;

use super::{Method, NoArgs, sealed};

// ---------------------------------------------------------------------------
// Request args
// ---------------------------------------------------------------------------

pub mod request {
    use super::NoArgs;

    /// Args for `fetchCanonicalManifest` — no parameters.
    pub type FetchCanonicalManifest = NoArgs;
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

pub mod response {
    use super::{Deserialize, Value};

    /// Reply for `fetchCanonicalManifest`.
    ///
    /// Firefox returns `{"manifest": <CanonicalManifest>, "from": …}` where the
    /// `manifest` object carries the parsed manifest (`values`), its
    /// conformance `errors`, and the resolved manifest `url`.  We keep it as a
    /// raw value; [`crate::actors::tab`] callers pick out the fields they
    /// surface.
    #[derive(Debug, Clone, Default, Deserialize)]
    pub struct FetchCanonicalManifest {
        #[serde(default)]
        pub manifest: Option<Value>,
    }
}

// ---------------------------------------------------------------------------
// Method markers
// ---------------------------------------------------------------------------

/// `fetchCanonicalManifest` method marker.
pub struct FetchCanonicalManifest;
impl sealed::Sealed for FetchCanonicalManifest {}
impl Method for FetchCanonicalManifest {
    const NAME: &'static str = "fetchCanonicalManifest";
    type Args = NoArgs;
    type Reply = response::FetchCanonicalManifest;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn method_name_is_correct() {
        assert_eq!(FetchCanonicalManifest::NAME, "fetchCanonicalManifest");
    }

    #[test]
    fn fetch_canonical_manifest_response_with_manifest() {
        let v = json!({
            "from": "server1.conn0.child1/manifestActor2",
            "manifest": {
                "url": "https://example.com/manifest.json",
                "values": {"name": "Example", "start_url": "/"},
                "errors": []
            }
        });
        let reply: response::FetchCanonicalManifest = serde_json::from_value(v).unwrap();
        let m = reply.manifest.expect("manifest present");
        assert_eq!(m["values"]["name"], "Example");
    }

    #[test]
    fn fetch_canonical_manifest_response_no_manifest() {
        // Firefox may return a manifest object whose `values` is null when the
        // page links no manifest — still a structurally-present `manifest` key.
        let v = json!({
            "from": "server1.conn0.child1/manifestActor2",
            "manifest": {"url": null, "values": null, "errors": []}
        });
        let reply: response::FetchCanonicalManifest = serde_json::from_value(v).unwrap();
        let m = reply.manifest.expect("manifest wrapper present");
        assert!(m["values"].is_null());
    }
}
