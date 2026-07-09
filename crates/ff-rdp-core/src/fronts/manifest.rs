//! Typed front for the Firefox `ManifestActor`.
//!
//! The manifest actor performs the WHATWG "obtain a manifest" algorithm and
//! returns the parsed Web App Manifest together with its conformance errors in
//! a single `fetchCanonicalManifest` call — a PWA-readiness audit primitive.
//!
//! The actor ID is exposed on the target frame (`manifestActor`) returned by
//! `getTarget`; see [`crate::actors::tab::TargetInfo::manifest_actor`].
//!
//! Mirrors <https://searchfox.org/mozilla-central/source/devtools/shared/specs/manifest.js>.

use serde_json::Value;

use crate::error::ProtocolError;
use crate::registry::{Front, FrontKind, Registry};
use crate::specs::{NoArgs, call, manifest as spec};
use crate::transport::RdpTransport;
use crate::types::ActorId;

/// The result of a `fetchCanonicalManifest` call.
///
/// A page with no linked manifest yields `manifest: None` (with `errors`
/// typically empty); a page with a manifest yields the parsed manifest object
/// under `manifest` plus any conformance `errors`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CanonicalManifest {
    /// The parsed manifest values (`name`, `start_url`, `icons`, …), or `None`
    /// when the page links no manifest.
    pub manifest: Option<Value>,
    /// The resolved manifest URL, when present.
    pub url: Option<String>,
    /// Conformance errors reported by the manifest processor.  Each entry is
    /// the raw error object Firefox produced (kept as-is so nothing is lost).
    pub errors: Vec<Value>,
}

/// A typed handle to a Firefox `Manifest` actor.
///
/// Creating a `ManifestFront` is O(1) and does not touch the network.
pub struct ManifestFront {
    id: ActorId,
    registry: Registry,
}

impl ManifestFront {
    /// Wrap an actor ID as a `ManifestFront` and register it.
    ///
    /// `target_root` is the owning target actor (the manifest actor lives under
    /// the tab target), used by the registry for invalidation on navigation.
    pub fn new(id: ActorId, registry: Registry, target_root: ActorId) -> Self {
        registry.register(id.clone(), FrontKind::Manifest, Some(target_root));
        Self { id, registry }
    }

    /// Fetch the canonical Web App Manifest for the current document.
    ///
    /// Returns a [`CanonicalManifest`] carrying the parsed manifest (or `None`
    /// when the page links none) and any conformance errors.  Tolerant of the
    /// exact wire shape: the parsed manifest lives under `values` (newer
    /// Firefox) or `manifest` (older); both are handled.
    pub fn fetch_canonical_manifest(
        &self,
        transport: &mut RdpTransport,
    ) -> Result<CanonicalManifest, ProtocolError> {
        let reply = call::<spec::FetchCanonicalManifest>(transport, &self.id, &NoArgs {})?;
        Ok(parse_canonical_manifest(reply.manifest.as_ref()))
    }
}

/// Project Firefox's raw `manifest` wrapper object into a [`CanonicalManifest`].
///
/// The wrapper carries the parsed manifest under `values` (or, on older
/// Firefox, `manifest`), the resolved `url`, and an `errors` array.  A page
/// with no manifest has a null/absent `values`, which we normalise to
/// `manifest: None`.
fn parse_canonical_manifest(wrapper: Option<&Value>) -> CanonicalManifest {
    let Some(wrapper) = wrapper.filter(|w| !w.is_null()) else {
        return CanonicalManifest::default();
    };

    // The parsed manifest lives under `values` (current) or `manifest` (older).
    let manifest = wrapper
        .get("values")
        .or_else(|| wrapper.get("manifest"))
        .filter(|v| !v.is_null())
        .cloned();

    let url = wrapper
        .get("url")
        .and_then(Value::as_str)
        .map(str::to_owned);

    let errors = wrapper
        .get("errors")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    CanonicalManifest {
        manifest,
        url,
        errors,
    }
}

impl Front for ManifestFront {
    fn id(&self) -> &ActorId {
        &self.id
    }

    fn registry(&self) -> &Registry {
        &self.registry
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::io::BufReader;
    use std::net::{TcpListener, TcpStream};

    use serde_json::json;

    use super::*;
    use crate::registry::Registry;
    use crate::transport::{RdpTransport, encode_frame, recv_from};

    fn make_transport_pair() -> (RdpTransport, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let client = TcpStream::connect(addr).unwrap();
        let (server, _) = listener.accept().unwrap();
        let writer = client.try_clone().unwrap();
        let reader = BufReader::new(client);
        (RdpTransport::from_parts(reader, writer), server)
    }

    #[allow(clippy::needless_pass_by_value)]
    fn server_reply(server: &TcpStream, msg: serde_json::Value) {
        use std::io::Write as _;
        let frame = encode_frame(&serde_json::to_string(&msg).unwrap());
        let mut s = server;
        s.write_all(frame.as_bytes()).unwrap();
    }

    fn server_read(server: &TcpStream) -> serde_json::Value {
        let mut reader = BufReader::new(server);
        recv_from(&mut reader).unwrap()
    }

    fn make_front(actor: &str) -> (ManifestFront, ActorId) {
        let id = ActorId::from(actor);
        let target_root = ActorId::from("server1.conn0.child1/windowGlobalTarget1");
        let front = ManifestFront::new(id.clone(), Registry::default(), target_root);
        (front, id)
    }

    #[test]
    fn fetch_canonical_manifest_parses_values_and_errors() {
        let (mut transport, server) = make_transport_pair();
        let (front, actor_id) = make_front("server1.conn0.child1/manifestActor2");

        let t = std::thread::spawn(move || {
            let req = server_read(&server);
            assert_eq!(req["to"], actor_id.as_ref());
            assert_eq!(req["type"], "fetchCanonicalManifest");
            server_reply(
                &server,
                json!({
                    "from": actor_id.as_ref(),
                    "manifest": {
                        "url": "https://example.com/manifest.json",
                        "values": {"name": "Example App", "start_url": "/app"},
                        "errors": [{"warn": "some warning"}]
                    }
                }),
            );
        });

        let result = front.fetch_canonical_manifest(&mut transport).unwrap();
        let manifest = result.manifest.expect("manifest present");
        assert_eq!(manifest["name"], "Example App");
        assert_eq!(manifest["start_url"], "/app");
        assert_eq!(
            result.url.as_deref(),
            Some("https://example.com/manifest.json")
        );
        assert_eq!(result.errors.len(), 1);
        t.join().unwrap();
    }

    #[test]
    fn fetch_canonical_manifest_no_manifest_is_none() {
        let (mut transport, server) = make_transport_pair();
        let (front, actor_id) = make_front("server1.conn0.child1/manifestActor3");

        let t = std::thread::spawn(move || {
            let _req = server_read(&server);
            server_reply(
                &server,
                json!({
                    "from": actor_id.as_ref(),
                    "manifest": {"url": null, "values": null, "errors": []}
                }),
            );
        });

        let result = front.fetch_canonical_manifest(&mut transport).unwrap();
        assert!(
            result.manifest.is_none(),
            "a page with no manifest yields manifest: None"
        );
        assert!(result.errors.is_empty());
        t.join().unwrap();
    }

    #[test]
    fn parse_canonical_manifest_handles_older_manifest_key() {
        // Older Firefox nests the parsed manifest under `manifest` instead of
        // `values`.
        let wrapper = json!({
            "url": "https://x/manifest.json",
            "manifest": {"name": "Legacy"},
            "errors": []
        });
        let result = parse_canonical_manifest(Some(&wrapper));
        assert_eq!(result.manifest.expect("manifest")["name"], "Legacy");
    }

    #[test]
    fn parse_canonical_manifest_null_wrapper_is_default() {
        assert_eq!(
            parse_canonical_manifest(Some(&Value::Null)),
            CanonicalManifest::default()
        );
        assert_eq!(parse_canonical_manifest(None), CanonicalManifest::default());
    }
}
