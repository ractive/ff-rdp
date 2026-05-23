//! Typed front for the Firefox `TargetConfigurationActor`.
//!
//! The target configuration actor accepts `updateConfiguration` requests that
//! change per-target settings such as cache behaviour, viewport size overrides,
//! and colour-scheme simulation.  It is obtained from `WatcherFront::get_target_configuration_actor`.
//!
//! Mirrors <https://searchfox.org/mozilla-central/source/devtools/shared/specs/target-configuration.js>

use serde_json::{Value, json};

use crate::actor::actor_request;
use crate::error::ProtocolError;
use crate::registry::{Front, FrontKind, Registry};
use crate::transport::RdpTransport;
use crate::types::ActorId;

/// A typed handle to a Firefox `TargetConfiguration` actor.
///
/// This actor lets the DevTools client change configuration for a specific
/// target (tab, worker, etc.) — cache disabling, viewport size overrides, and
/// colour-scheme simulation — without requiring browser-wide pref changes.
///
/// Creating a `TargetConfigurationFront` is O(1) and does not touch the network.
pub struct TargetConfigurationFront {
    id: ActorId,
    registry: Registry,
}

impl TargetConfigurationFront {
    /// Wrap an actor ID as a `TargetConfigurationFront` and register it.
    pub fn new(id: ActorId, registry: Registry, target_root: ActorId) -> Self {
        registry.register(
            id.clone(),
            FrontKind::TargetConfiguration,
            Some(target_root),
        );
        Self { id, registry }
    }

    /// Disable (or re-enable) the HTTP cache for this target.
    ///
    /// When `disabled` is `true`, all cached responses are ignored and every
    /// request hits the network.  This is equivalent to the "Disable Cache"
    /// checkbox in the Firefox DevTools Network panel.
    pub fn set_cache_disabled(
        &self,
        transport: &mut RdpTransport,
        disabled: bool,
    ) -> Result<(), ProtocolError> {
        self.update_configuration(transport, &json!({"cacheDisabled": disabled}))
    }

    /// Set the preferred colour scheme for this target.
    ///
    /// `scheme` should be `"light"`, `"dark"`, or `"no-preference"`.
    /// Useful for testing `@media (prefers-color-scheme: dark)` rules.
    pub fn set_color_scheme_simulation(
        &self,
        transport: &mut RdpTransport,
        scheme: &str,
    ) -> Result<(), ProtocolError> {
        self.update_configuration(transport, &json!({"colorScheme": scheme}))
    }

    /// Override the viewport size for this target (in CSS pixels).
    ///
    /// Set `width` and `height` to the desired viewport dimensions.
    /// Pass `0` for both to reset to the natural viewport size.
    pub fn set_custom_viewport_size(
        &self,
        transport: &mut RdpTransport,
        width: u32,
        height: u32,
    ) -> Result<(), ProtocolError> {
        self.update_configuration(
            transport,
            &json!({"customViewport": {"width": width, "height": height}}),
        )
    }

    /// Send an `updateConfiguration` request with the given configuration patch.
    ///
    /// The patch is merged into the target's live configuration on the Firefox
    /// side.  Unknown keys are silently ignored by the actor.
    fn update_configuration(
        &self,
        transport: &mut RdpTransport,
        configuration: &Value,
    ) -> Result<(), ProtocolError> {
        let params = json!({"configuration": configuration});
        actor_request(
            transport,
            self.id.as_ref(),
            "updateConfiguration",
            Some(&params),
        )?;
        Ok(())
    }
}

impl Front for TargetConfigurationFront {
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

    fn make_front(actor: &str) -> (TargetConfigurationFront, ActorId) {
        let id = ActorId::from(actor);
        let target_root = ActorId::from("server1.conn0.child1/windowGlobalTarget1");
        let front = TargetConfigurationFront::new(id.clone(), Registry::default(), target_root);
        (front, id)
    }

    #[test]
    fn set_cache_disabled_sends_correct_request() {
        let (mut transport, server) = make_transport_pair();
        let (front, actor_id) = make_front("server1.conn0.targetConf1");

        let t = std::thread::spawn(move || {
            let req = server_read(&server);
            assert_eq!(req["to"], actor_id.as_ref());
            assert_eq!(req["type"], "updateConfiguration");
            assert_eq!(req["configuration"]["cacheDisabled"], true);
            server_reply(&server, json!({"from": actor_id.as_ref()}));
        });

        front.set_cache_disabled(&mut transport, true).unwrap();
        t.join().unwrap();
    }

    #[test]
    fn set_cache_disabled_false_sends_false() {
        let (mut transport, server) = make_transport_pair();
        let (front, actor_id) = make_front("server1.conn0.targetConf2");

        let t = std::thread::spawn(move || {
            let req = server_read(&server);
            assert_eq!(req["configuration"]["cacheDisabled"], false);
            server_reply(&server, json!({"from": actor_id.as_ref()}));
        });

        front.set_cache_disabled(&mut transport, false).unwrap();
        t.join().unwrap();
    }

    #[test]
    fn set_color_scheme_simulation_sends_scheme() {
        let (mut transport, server) = make_transport_pair();
        let (front, actor_id) = make_front("server1.conn0.targetConf3");

        let t = std::thread::spawn(move || {
            let req = server_read(&server);
            assert_eq!(req["type"], "updateConfiguration");
            assert_eq!(req["configuration"]["colorScheme"], "dark");
            server_reply(&server, json!({"from": actor_id.as_ref()}));
        });

        front
            .set_color_scheme_simulation(&mut transport, "dark")
            .unwrap();
        t.join().unwrap();
    }

    #[test]
    fn set_custom_viewport_size_sends_dimensions() {
        let (mut transport, server) = make_transport_pair();
        let (front, actor_id) = make_front("server1.conn0.targetConf4");

        let t = std::thread::spawn(move || {
            let req = server_read(&server);
            assert_eq!(req["type"], "updateConfiguration");
            assert_eq!(req["configuration"]["customViewport"]["width"], 1280);
            assert_eq!(req["configuration"]["customViewport"]["height"], 800);
            server_reply(&server, json!({"from": actor_id.as_ref()}));
        });

        front
            .set_custom_viewport_size(&mut transport, 1280, 800)
            .unwrap();
        t.join().unwrap();
    }
}
