//! Typed front for the Firefox `TargetConfigurationActor`.
//!
//! The target configuration actor accepts `updateConfiguration` requests that
//! change per-target settings such as cache behaviour, custom user agent,
//! device-pixel-ratio override, print-media simulation, touch-event override,
//! JavaScript-disabled testing, tab-offline emulation, and colour-scheme
//! simulation.  It is obtained from `WatcherFront::get_target_configuration_actor`.
//!
//! Every field in the server's `SUPPORTED_OPTIONS` dict is `nullable:*`, so a
//! partial patch only touches the keys it names — this front therefore exposes
//! one setter per option plus a [`TargetConfigurationFront::reset`] that sends
//! the documented "restore defaults" value for each field in a single request.
//!
//! Mirrors <https://searchfox.org/mozilla-central/source/devtools/shared/specs/target-configuration.js>
//! (`target-configuration.configuration` dict, verified against the Firefox
//! checkout during iter-103).

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
    /// `scheme` should be `"light"`, `"dark"`, or `"none"` (system default).
    /// Useful for testing `@media (prefers-color-scheme: dark)` rules.
    ///
    /// The wire field is `colorSchemeSimulation` (verified against the server's
    /// `SUPPORTED_OPTIONS` dict); the browsing context maps the value onto
    /// `prefersColorSchemeOverride`.
    pub fn set_color_scheme_simulation(
        &self,
        transport: &mut RdpTransport,
        scheme: &str,
    ) -> Result<(), ProtocolError> {
        self.update_configuration(transport, &json!({"colorSchemeSimulation": scheme}))
    }

    /// Override the reported user agent for this target.
    ///
    /// Sets `navigator.userAgent` and the `User-Agent` request header for
    /// subsequent loads.  Pass an empty string to restore the original UA.
    pub fn set_custom_user_agent(
        &self,
        transport: &mut RdpTransport,
        user_agent: &str,
    ) -> Result<(), ProtocolError> {
        self.update_configuration(transport, &json!({"customUserAgent": user_agent}))
    }

    /// Override the device pixel ratio (`window.devicePixelRatio`) for this target.
    ///
    /// The wire field is `overrideDPPX` (device pixels per CSS px).  A value of
    /// `0.0` clears the override and restores the physical ratio.
    pub fn set_override_dppx(
        &self,
        transport: &mut RdpTransport,
        dppx: f64,
    ) -> Result<(), ProtocolError> {
        self.update_configuration(transport, &json!({"overrideDPPX": dppx}))
    }

    /// Enable or disable print-media simulation for this target.
    ///
    /// When enabled the page renders as if `@media print` were active — useful
    /// for auditing print stylesheets (compose with `screenshot`).
    pub fn set_print_simulation_enabled(
        &self,
        transport: &mut RdpTransport,
        enabled: bool,
    ) -> Result<(), ProtocolError> {
        self.update_configuration(transport, &json!({"printSimulationEnabled": enabled}))
    }

    /// Enable or disable touch-event simulation for this target.
    ///
    /// The wire field is `touchEventsOverride`, a string enum: `"enabled"`
    /// turns touch simulation on, `"none"` restores the default.  When `on` is
    /// `true` the value `"enabled"` is sent; when `false`, `"none"`.
    pub fn set_touch_events_override(
        &self,
        transport: &mut RdpTransport,
        on: bool,
    ) -> Result<(), ProtocolError> {
        let value = if on { "enabled" } else { "none" };
        self.update_configuration(transport, &json!({"touchEventsOverride": value}))
    }

    /// Enable or disable JavaScript execution for this target.
    ///
    /// `enabled = false` disables scripting.  The server reloads the document
    /// when this flag changes, so callers must reload (or wait for the
    /// server-initiated reload) before probing the effect.
    pub fn set_javascript_enabled(
        &self,
        transport: &mut RdpTransport,
        enabled: bool,
    ) -> Result<(), ProtocolError> {
        self.update_configuration(transport, &json!({"javascriptEnabled": enabled}))
    }

    /// Put the target's tab into (or out of) offline mode.
    ///
    /// When `offline` is `true`, network requests fail and
    /// `navigator.onLine` reports `false` — useful for exercising PWA/offline
    /// UX.  A reload is generally required for `navigator.onLine` to update.
    pub fn set_tab_offline(
        &self,
        transport: &mut RdpTransport,
        offline: bool,
    ) -> Result<(), ProtocolError> {
        self.update_configuration(transport, &json!({"setTabOffline": offline}))
    }

    /// Restore every configurable option to its documented default in a single
    /// `updateConfiguration` request.
    ///
    /// The reset values mirror the server's teardown logic
    /// (`_onTargetConfigurationDestroy` in `target-configuration.js`):
    ///
    /// | field                   | reset value |
    /// |-------------------------|-------------|
    /// | `cacheDisabled`         | `false`     |
    /// | `colorSchemeSimulation` | `"none"`    |
    /// | `customUserAgent`       | `""`        |
    /// | `overrideDPPX`          | `0`         |
    /// | `printSimulationEnabled`| `false`     |
    /// | `touchEventsOverride`   | `"none"`    |
    /// | `javascriptEnabled`     | `true`      |
    /// | `setTabOffline`         | `false`     |
    ///
    /// Note: a full reset re-enables JavaScript, which triggers a
    /// server-side reload if scripting had been disabled.
    pub fn reset(&self, transport: &mut RdpTransport) -> Result<(), ProtocolError> {
        self.update_configuration(
            transport,
            &json!({
                "cacheDisabled": false,
                "colorSchemeSimulation": "none",
                "customUserAgent": "",
                "overrideDPPX": 0,
                "printSimulationEnabled": false,
                "touchEventsOverride": "none",
                "javascriptEnabled": true,
                "setTabOffline": false,
            }),
        )
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
            // The server dict field is `colorSchemeSimulation`, not `colorScheme`.
            assert_eq!(req["configuration"]["colorSchemeSimulation"], "dark");
            server_reply(&server, json!({"from": actor_id.as_ref()}));
        });

        front
            .set_color_scheme_simulation(&mut transport, "dark")
            .unwrap();
        t.join().unwrap();
    }

    #[test]
    fn set_custom_user_agent_sends_string() {
        let (mut transport, server) = make_transport_pair();
        let (front, actor_id) = make_front("server1.conn0.targetConf5");

        let t = std::thread::spawn(move || {
            let req = server_read(&server);
            assert_eq!(req["type"], "updateConfiguration");
            assert_eq!(req["configuration"]["customUserAgent"], "ff-rdp-test/1.0");
            server_reply(&server, json!({"from": actor_id.as_ref()}));
        });

        front
            .set_custom_user_agent(&mut transport, "ff-rdp-test/1.0")
            .unwrap();
        t.join().unwrap();
    }

    #[test]
    fn set_override_dppx_sends_number() {
        let (mut transport, server) = make_transport_pair();
        let (front, actor_id) = make_front("server1.conn0.targetConf6");

        let t = std::thread::spawn(move || {
            let req = server_read(&server);
            assert_eq!(req["type"], "updateConfiguration");
            assert_eq!(req["configuration"]["overrideDPPX"], 2.0);
            server_reply(&server, json!({"from": actor_id.as_ref()}));
        });

        front.set_override_dppx(&mut transport, 2.0).unwrap();
        t.join().unwrap();
    }

    #[test]
    fn set_print_simulation_enabled_sends_bool() {
        let (mut transport, server) = make_transport_pair();
        let (front, actor_id) = make_front("server1.conn0.targetConf7");

        let t = std::thread::spawn(move || {
            let req = server_read(&server);
            assert_eq!(req["configuration"]["printSimulationEnabled"], true);
            server_reply(&server, json!({"from": actor_id.as_ref()}));
        });

        front
            .set_print_simulation_enabled(&mut transport, true)
            .unwrap();
        t.join().unwrap();
    }

    #[test]
    fn set_touch_events_override_maps_bool_to_enum() {
        let (mut transport, server) = make_transport_pair();
        let (front, actor_id) = make_front("server1.conn0.targetConf8");

        let t = std::thread::spawn(move || {
            // on == true -> "enabled"
            let req = server_read(&server);
            assert_eq!(req["configuration"]["touchEventsOverride"], "enabled");
            server_reply(&server, json!({"from": actor_id.as_ref()}));
            // off == false -> "none"
            let req = server_read(&server);
            assert_eq!(req["configuration"]["touchEventsOverride"], "none");
            server_reply(&server, json!({"from": actor_id.as_ref()}));
        });

        front
            .set_touch_events_override(&mut transport, true)
            .unwrap();
        front
            .set_touch_events_override(&mut transport, false)
            .unwrap();
        t.join().unwrap();
    }

    #[test]
    fn set_javascript_enabled_sends_bool() {
        let (mut transport, server) = make_transport_pair();
        let (front, actor_id) = make_front("server1.conn0.targetConf9");

        let t = std::thread::spawn(move || {
            let req = server_read(&server);
            assert_eq!(req["configuration"]["javascriptEnabled"], false);
            server_reply(&server, json!({"from": actor_id.as_ref()}));
        });

        front.set_javascript_enabled(&mut transport, false).unwrap();
        t.join().unwrap();
    }

    #[test]
    fn set_tab_offline_sends_bool() {
        let (mut transport, server) = make_transport_pair();
        let (front, actor_id) = make_front("server1.conn0.targetConf10");

        let t = std::thread::spawn(move || {
            let req = server_read(&server);
            assert_eq!(req["configuration"]["setTabOffline"], true);
            server_reply(&server, json!({"from": actor_id.as_ref()}));
        });

        front.set_tab_offline(&mut transport, true).unwrap();
        t.join().unwrap();
    }

    #[test]
    fn reset_sends_all_defaults() {
        let (mut transport, server) = make_transport_pair();
        let (front, actor_id) = make_front("server1.conn0.targetConf11");

        let t = std::thread::spawn(move || {
            let req = server_read(&server);
            assert_eq!(req["type"], "updateConfiguration");
            let cfg = &req["configuration"];
            assert_eq!(cfg["cacheDisabled"], false);
            assert_eq!(cfg["colorSchemeSimulation"], "none");
            assert_eq!(cfg["customUserAgent"], "");
            assert_eq!(cfg["overrideDPPX"], 0);
            assert_eq!(cfg["printSimulationEnabled"], false);
            assert_eq!(cfg["touchEventsOverride"], "none");
            assert_eq!(cfg["javascriptEnabled"], true);
            assert_eq!(cfg["setTabOffline"], false);
            server_reply(&server, json!({"from": actor_id.as_ref()}));
        });

        front.reset(&mut transport).unwrap();
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
