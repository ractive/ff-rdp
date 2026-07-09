//! Typed front for the Firefox `NetworkParentActor`.
//!
//! The network-parent actor is the parent-process entry point for
//! *configuring* network behaviour for a debugging session — as opposed to the
//! per-request `NetworkEventActor`s, which only *observe* traffic. It exposes
//! request throttling (`setNetworkThrottling`) and URL blocking
//! (`setBlockedUrls`). It is obtained from
//! [`WatcherFront::get_network_parent_actor`](crate::WatcherFront::get_network_parent_actor).
//!
//! # Protocol quirk (matched-request, not oneway)
//!
//! `setNetworkThrottling` / `setBlockedUrls` declare **no response block** in
//! `devtools/shared/specs/network-parent.js`, but they are **not** marked
//! `oneway`. Firefox therefore still sends an (empty) reply packet that the
//! client must read — exactly the same shape as `walker.releaseNode`. These
//! methods use the ordinary [`actor_request`](crate::actor::actor_request)
//! path (which reads the reply), **not** the fire-and-forget `actor_send` used
//! for genuinely-oneway methods like `clearResources`.
//!
//! # Prerequisite
//!
//! Every network-parent method throws `"Not listening for network events"`
//! unless `watchResources(["network-event"])` has been issued on the owning
//! watcher first. Callers must subscribe to the network-event resource before
//! driving this front.
//!
//! Mirrors <https://searchfox.org/mozilla-central/source/devtools/shared/specs/network-parent.js>
//! (verified against `kb/rdp/actors/network-parent.md`).

use serde_json::{Value, json};

use crate::actor::actor_request;
use crate::error::ProtocolError;
use crate::registry::{Front, FrontKind, Registry};
use crate::transport::RdpTransport;
use crate::types::ActorId;

/// A well-known network-throttling profile.
///
/// The wire values are `(latency_ms, download_bps, upload_bps)`, where the
/// throughput fields are **bytes per second** (despite Firefox's UI labelling
/// them in bits). The presets mirror the canonical DevTools "Slow 3G" and
/// "Fast 3G" tiers so callers get behaviour that matches other browsers'
/// tooling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThrottleProfile {
    /// Slow 3G: ~400 kbit/s down, ~400 kbit/s up, 400 ms round-trip latency.
    Slow3g,
    /// Fast 3G: ~1.6 Mbit/s down, ~750 kbit/s up, 150 ms round-trip latency.
    Fast3g,
}

impl ThrottleProfile {
    /// Round-trip latency to inject, in milliseconds.
    pub fn latency_ms(self) -> u64 {
        match self {
            // Canonical DevTools presets (Chrome/Lighthouse "Slow 3G" /
            // "Fast 3G"); Firefox's own netmonitor presets are in the same
            // ballpark. Latency is the added round-trip time in ms.
            ThrottleProfile::Slow3g => 400,
            ThrottleProfile::Fast3g => 150,
        }
    }

    /// Download throughput cap, in **bytes per second**.
    pub fn download_bps(self) -> u64 {
        match self {
            // 400 kbit/s and 1.6 Mbit/s expressed in bytes/sec.
            ThrottleProfile::Slow3g => 50_000,
            ThrottleProfile::Fast3g => 200_000,
        }
    }

    /// Upload throughput cap, in **bytes per second**.
    pub fn upload_bps(self) -> u64 {
        match self {
            ThrottleProfile::Slow3g => 50_000,
            ThrottleProfile::Fast3g => 93_750,
        }
    }

    /// The stable string identifier used on the CLI and echoed in the envelope.
    pub fn as_str(self) -> &'static str {
        match self {
            ThrottleProfile::Slow3g => "slow-3g",
            ThrottleProfile::Fast3g => "fast-3g",
        }
    }
}

/// A typed handle to a Firefox `NetworkParent` actor.
///
/// This actor configures parent-process network behaviour for the debugging
/// session: request throttling and URL blocking. Creating a
/// `NetworkParentFront` is O(1) and does not touch the network.
pub struct NetworkParentFront {
    id: ActorId,
    registry: Registry,
}

impl NetworkParentFront {
    /// Wrap an actor ID as a `NetworkParentFront` and register it.
    ///
    /// `watcher_root` is the owning watcher actor; it is recorded as the front's
    /// target root so the registry can cascade invalidation when the watcher
    /// (or its target) is torn down.
    pub fn new(id: ActorId, registry: Registry, watcher_root: ActorId) -> Self {
        registry.register(id.clone(), FrontKind::NetworkParent, Some(watcher_root));
        Self { id, registry }
    }

    /// Apply a network-throttling profile to the session.
    ///
    /// Sends `setNetworkThrottling({latency, downloadThroughput,
    /// uploadThroughput})`. `latency` is milliseconds; the throughput fields are
    /// bytes per second. Firefox expands these internally to its
    /// `*Mean`/`*Max` pair.
    pub fn set_network_throttling(
        &self,
        transport: &mut RdpTransport,
        profile: ThrottleProfile,
    ) -> Result<(), ProtocolError> {
        let options = json!({
            "latency": profile.latency_ms(),
            "downloadThroughput": profile.download_bps(),
            "uploadThroughput": profile.upload_bps(),
        });
        self.send_throttling(transport, &options)
    }

    /// Clear any active throttling, restoring full-speed network behaviour.
    ///
    /// Firefox treats `setNetworkThrottling(null)` as "restore the defaults
    /// captured on the first set" (see `network-parent.js`).
    pub fn clear_network_throttling(
        &self,
        transport: &mut RdpTransport,
    ) -> Result<(), ProtocolError> {
        self.send_throttling(transport, &Value::Null)
    }

    /// Replace the session's URL block-list.
    ///
    /// Every request whose URL matches one of `urls` (Firefox performs a
    /// substring/glob match) is failed with `NS_ERROR_ABORT`. Passing an empty
    /// slice clears the block-list.
    pub fn set_blocked_urls(
        &self,
        transport: &mut RdpTransport,
        urls: &[String],
    ) -> Result<(), ProtocolError> {
        // `setBlockedUrls` declares no response block but is NOT oneway —
        // Firefox still sends an empty ACK we must read. Use `actor_request`.
        let params = json!({ "urls": urls });
        actor_request(transport, self.id.as_ref(), "setBlockedUrls", Some(&params))?;
        Ok(())
    }

    /// Send `setNetworkThrottling` with the given `options` value (an object or
    /// `null`).
    ///
    /// Like `setBlockedUrls`, this method is response-less but not oneway, so we
    /// read the empty ACK via `actor_request`.
    fn send_throttling(
        &self,
        transport: &mut RdpTransport,
        options: &Value,
    ) -> Result<(), ProtocolError> {
        let params = json!({ "options": options });
        actor_request(
            transport,
            self.id.as_ref(),
            "setNetworkThrottling",
            Some(&params),
        )?;
        Ok(())
    }
}

impl Front for NetworkParentFront {
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

    fn make_front(actor: &str) -> (NetworkParentFront, ActorId) {
        let id = ActorId::from(actor);
        let watcher_root = ActorId::from("server1.conn0.watcher4");
        let front = NetworkParentFront::new(id.clone(), Registry::default(), watcher_root);
        (front, id)
    }

    #[test]
    fn slow3g_profile_values() {
        let p = ThrottleProfile::Slow3g;
        assert_eq!(p.as_str(), "slow-3g");
        assert_eq!(p.latency_ms(), 400);
        assert_eq!(p.download_bps(), 50_000);
        assert_eq!(p.upload_bps(), 50_000);
    }

    #[test]
    fn fast3g_profile_values() {
        let p = ThrottleProfile::Fast3g;
        assert_eq!(p.as_str(), "fast-3g");
        assert_eq!(p.latency_ms(), 150);
        assert_eq!(p.download_bps(), 200_000);
        assert_eq!(p.upload_bps(), 93_750);
    }

    #[test]
    fn fast3g_is_faster_than_slow3g() {
        // The AC relies on slow-3g being measurably slower than baseline; the
        // ordering between tiers is a sanity check that the constants weren't
        // swapped.
        assert!(ThrottleProfile::Fast3g.download_bps() > ThrottleProfile::Slow3g.download_bps());
        assert!(ThrottleProfile::Fast3g.latency_ms() < ThrottleProfile::Slow3g.latency_ms());
    }

    #[test]
    fn set_network_throttling_sends_options_object() {
        let (mut transport, server) = make_transport_pair();
        let (front, actor_id) = make_front("server1.conn0.networkParent5");

        let t = std::thread::spawn(move || {
            let req = server_read(&server);
            assert_eq!(req["to"], actor_id.as_ref());
            assert_eq!(req["type"], "setNetworkThrottling");
            let opts = &req["options"];
            assert_eq!(opts["latency"], 400);
            assert_eq!(opts["downloadThroughput"], 50_000);
            assert_eq!(opts["uploadThroughput"], 50_000);
            // Response-less-but-not-oneway: the server sends an empty ACK the
            // client must read.
            server_reply(&server, json!({"from": actor_id.as_ref()}));
        });

        front
            .set_network_throttling(&mut transport, ThrottleProfile::Slow3g)
            .unwrap();
        t.join().unwrap();
    }

    #[test]
    fn clear_network_throttling_sends_null_options() {
        let (mut transport, server) = make_transport_pair();
        let (front, actor_id) = make_front("server1.conn0.networkParent6");

        let t = std::thread::spawn(move || {
            let req = server_read(&server);
            assert_eq!(req["type"], "setNetworkThrottling");
            assert!(
                req["options"].is_null(),
                "clear must send options: null, got {}",
                req["options"]
            );
            server_reply(&server, json!({"from": actor_id.as_ref()}));
        });

        front.clear_network_throttling(&mut transport).unwrap();
        t.join().unwrap();
    }

    #[test]
    fn set_blocked_urls_sends_url_array() {
        let (mut transport, server) = make_transport_pair();
        let (front, actor_id) = make_front("server1.conn0.networkParent7");

        let t = std::thread::spawn(move || {
            let req = server_read(&server);
            assert_eq!(req["type"], "setBlockedUrls");
            assert_eq!(req["urls"], json!(["*.png", "ads.example.com"]));
            server_reply(&server, json!({"from": actor_id.as_ref()}));
        });

        front
            .set_blocked_urls(
                &mut transport,
                &["*.png".to_owned(), "ads.example.com".to_owned()],
            )
            .unwrap();
        t.join().unwrap();
    }

    #[test]
    fn set_blocked_urls_empty_clears_list() {
        let (mut transport, server) = make_transport_pair();
        let (front, actor_id) = make_front("server1.conn0.networkParent8");

        let t = std::thread::spawn(move || {
            let req = server_read(&server);
            assert_eq!(req["type"], "setBlockedUrls");
            assert_eq!(req["urls"], json!([]));
            server_reply(&server, json!({"from": actor_id.as_ref()}));
        });

        front.set_blocked_urls(&mut transport, &[]).unwrap();
        t.join().unwrap();
    }
}
