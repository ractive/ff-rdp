use serde_json::Value;

use crate::error::{ActorErrorKind, ProtocolError};
use crate::registry::{Front, FrontKind, Registry};
use crate::specs::{NoArgs, call, watcher as spec};
use crate::transport::RdpTransport;
use crate::types::ActorId;

/// A typed handle to a Firefox Watcher actor.
///
/// Watcher actors manage resource subscriptions (`watchResources`, `watchTargets`)
/// and deliver push events for network, console, and target lifecycle events.
///
/// Creating a `WatcherFront` is O(1) and does not touch the network.
pub struct WatcherFront {
    id: ActorId,
    registry: Registry,
    /// The owning target root (if this watcher is scoped to a tab).
    target_root: Option<ActorId>,
}

impl WatcherFront {
    /// Wrap an actor ID as a `WatcherFront` and register it in the registry.
    pub fn new(id: ActorId, registry: Registry, target_root: Option<ActorId>) -> Self {
        registry.register(id.clone(), FrontKind::Watcher, target_root.clone());
        Self {
            id,
            registry,
            target_root,
        }
    }

    /// The owning target root, if any.
    pub fn target_root(&self) -> Option<&ActorId> {
        self.target_root.as_ref()
    }

    /// Subscribe to one or more resource types.
    ///
    /// After calling this, Firefox will send `resources-available-array` events
    /// for both existing and new resources of the requested types.
    ///
    /// Resource types: `"network-event"`, `"console-message"`, `"error-message"`, etc.
    ///
    /// Uses an explicit filtered recv loop to skip any `resources-available-array`
    /// push events (which carry a `type` field) that Firefox may send before the
    /// ACK from the watcher actor.
    pub fn watch_resources(
        &self,
        transport: &mut RdpTransport,
        types: &[&str],
    ) -> Result<(), ProtocolError> {
        let args = spec::request::WatchResources {
            resource_types: types.iter().map(|s| (*s).to_string()).collect(),
        };
        let params = serde_json::to_value(&args)
            .map_err(|e| ProtocolError::InvalidPacket(format!("encode watchResources: {e}")))?;
        self.send_and_wait_ack(transport, "watchResources", params)
    }

    /// Unsubscribe from one or more resource types.
    ///
    /// Uses an explicit filtered recv loop to skip push events before the ACK.
    pub fn unwatch_resources(
        &self,
        transport: &mut RdpTransport,
        types: &[&str],
    ) -> Result<(), ProtocolError> {
        let args = spec::request::UnwatchResources {
            resource_types: types.iter().map(|s| (*s).to_string()).collect(),
        };
        let params = serde_json::to_value(&args)
            .map_err(|e| ProtocolError::InvalidPacket(format!("encode unwatchResources: {e}")))?;
        self.send_and_wait_ack(transport, "unwatchResources", params)
    }

    /// Subscribe to target events of the given type.
    ///
    /// Target types: `"frame"`, `"worker"`, `"process"`, etc.
    ///
    /// Uses an explicit filtered recv loop to skip push events before the ACK.
    pub fn watch_targets(
        &self,
        transport: &mut RdpTransport,
        target_type: &str,
    ) -> Result<(), ProtocolError> {
        let args = spec::request::WatchTargets {
            target_type: target_type.to_string(),
        };
        let params = serde_json::to_value(&args)
            .map_err(|e| ProtocolError::InvalidPacket(format!("encode watchTargets: {e}")))?;
        self.send_and_wait_ack(transport, "watchTargets", params)
    }

    /// Unsubscribe from target events of the given type.
    ///
    /// `unwatchTargets` is oneway in Firefox's spec — no reply packet is sent.
    /// The request is dispatched via the spec-layer [`call`] helper which skips
    /// the reply read when `Method::ONEWAY` is `true`, preventing a hang on
    /// CLI shutdown.
    pub fn unwatch_targets(
        &self,
        transport: &mut RdpTransport,
        target_type: &str,
    ) -> Result<(), ProtocolError> {
        let args = spec::request::UnwatchTargets {
            target_type: target_type.to_string(),
        };
        call::<spec::UnwatchTargets>(transport, &self.id, &args)?;
        Ok(())
    }

    /// Clear cached resources for the given resource types.
    ///
    /// `clearResources` is oneway in Firefox's spec — no reply is expected.
    pub fn clear_resources(
        &self,
        transport: &mut RdpTransport,
        resource_types: &[&str],
    ) -> Result<(), ProtocolError> {
        let args = spec::request::ClearResources {
            resource_types: resource_types.iter().map(|s| (*s).to_string()).collect(),
        };
        call::<spec::ClearResources>(transport, &self.id, &args)?;
        Ok(())
    }

    /// Get the parent browsing context ID for this watcher's scope.
    pub fn get_parent_browsing_context_id(
        &self,
        transport: &mut RdpTransport,
    ) -> Result<Option<u64>, ProtocolError> {
        let reply = call::<spec::GetParentBrowsingContextId>(transport, &self.id, &NoArgs {})?;
        Ok(reply.browsing_context_id)
    }

    /// Get the network parent actor for this watcher's scope.
    ///
    /// This actor is the entry point for CORS-aware response-body fetching.
    pub fn get_network_parent_actor(
        &self,
        transport: &mut RdpTransport,
    ) -> Result<ActorId, ProtocolError> {
        let reply = call::<spec::GetNetworkParentActor>(transport, &self.id, &NoArgs {})?;
        Ok(reply.actor)
    }

    /// Get the blackboxing actor for this watcher's scope.
    pub fn get_blackboxing_actor(
        &self,
        transport: &mut RdpTransport,
    ) -> Result<ActorId, ProtocolError> {
        let reply = call::<spec::GetBlackboxingActor>(transport, &self.id, &NoArgs {})?;
        Ok(reply.actor)
    }

    /// Get the breakpoint list actor for this watcher's scope.
    pub fn get_breakpoint_list_actor(
        &self,
        transport: &mut RdpTransport,
    ) -> Result<ActorId, ProtocolError> {
        let reply = call::<spec::GetBreakpointListActor>(transport, &self.id, &NoArgs {})?;
        Ok(reply.actor)
    }

    /// Get the target configuration actor for this watcher's scope.
    ///
    /// The `TargetConfigurationFront` obtained from this actor can set
    /// `cacheDisabled`, viewport size overrides, and colour-scheme simulation.
    pub fn get_target_configuration_actor(
        &self,
        transport: &mut RdpTransport,
    ) -> Result<ActorId, ProtocolError> {
        let reply = call::<spec::GetTargetConfigurationActor>(transport, &self.id, &NoArgs {})?;
        Ok(reply.actor)
    }

    /// Get the thread configuration actor for this watcher's scope.
    pub fn get_thread_configuration_actor(
        &self,
        transport: &mut RdpTransport,
    ) -> Result<ActorId, ProtocolError> {
        let reply = call::<spec::GetThreadConfigurationActor>(transport, &self.id, &NoArgs {})?;
        Ok(reply.actor)
    }

    /// Send a watcher request and wait for the ACK, skipping any push events
    /// (packets from this actor that carry a `type` field) that arrive before it.
    fn send_and_wait_ack(
        &self,
        transport: &mut RdpTransport,
        method: &str,
        mut params: Value,
    ) -> Result<(), ProtocolError> {
        // Build the request: merge `to` and `type` into the params object.
        let obj = params.as_object_mut().ok_or_else(|| {
            ProtocolError::InvalidPacket("watcher request params must be a JSON object".into())
        })?;
        obj.insert("to".into(), serde_json::json!(self.id.as_ref()));
        obj.insert("type".into(), serde_json::json!(method));

        transport.send(&params)?;

        // Loop until we find the ACK: a packet from this actor with no `type`.
        loop {
            let msg = transport.recv()?;
            let from = msg.get("from").and_then(Value::as_str).unwrap_or_default();
            if from == self.id.as_ref() {
                if msg.get("type").is_some() {
                    // Push event (e.g. resources-available-array) — skip it.
                    continue;
                }
                if let Some(error) = msg.get("error").and_then(Value::as_str) {
                    return Err(ProtocolError::ActorError {
                        actor: self.id.as_ref().to_owned(),
                        kind: ActorErrorKind::from_code(error),
                        error: error.to_owned(),
                        message: msg
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_owned(),
                    });
                }
                // ACK received.
                return Ok(());
            }
        }
    }
}

impl Front for WatcherFront {
    fn id(&self) -> &ActorId {
        &self.id
    }

    fn registry(&self) -> &Registry {
        &self.registry
    }
}

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

    #[test]
    fn watch_resources_sends_correct_request() {
        let (mut transport, server) = make_transport_pair();
        let front = WatcherFront::new(
            ActorId::from("server1.conn0.watcher4"),
            Registry::default(),
            None,
        );

        let t = std::thread::spawn(move || {
            let req = server_read(&server);
            assert_eq!(req["to"], "server1.conn0.watcher4");
            assert_eq!(req["type"], "watchResources");
            assert_eq!(req["resourceTypes"], json!(["network-event"]));
            server_reply(&server, json!({"from": "server1.conn0.watcher4"}));
        });

        front
            .watch_resources(&mut transport, &["network-event"])
            .unwrap();
        t.join().unwrap();
    }

    #[test]
    fn watch_resources_skips_push_event_before_ack() {
        let (mut transport, server) = make_transport_pair();
        let front = WatcherFront::new(
            ActorId::from("server1.conn0.watcher4"),
            Registry::default(),
            None,
        );

        let t = std::thread::spawn(move || {
            let _req = server_read(&server);
            // Push event arriving before the ACK — should be skipped.
            server_reply(
                &server,
                json!({"from": "server1.conn0.watcher4", "type": "resources-available-array", "array": []}),
            );
            // Real ACK (no `type` field).
            server_reply(&server, json!({"from": "server1.conn0.watcher4"}));
        });

        front
            .watch_resources(&mut transport, &["network-event"])
            .unwrap();
        t.join().unwrap();
    }

    #[test]
    fn watch_targets_sends_target_type() {
        let (mut transport, server) = make_transport_pair();
        let front = WatcherFront::new(
            ActorId::from("server1.conn0.watcher4"),
            Registry::default(),
            None,
        );

        let t = std::thread::spawn(move || {
            let req = server_read(&server);
            assert_eq!(req["type"], "watchTargets");
            assert_eq!(req["targetType"], "frame");
            server_reply(&server, json!({"from": "server1.conn0.watcher4"}));
        });

        front.watch_targets(&mut transport, "frame").unwrap();
        t.join().unwrap();
    }

    #[test]
    fn unwatch_resources_sends_correct_request() {
        let (mut transport, server) = make_transport_pair();
        let front = WatcherFront::new(
            ActorId::from("server1.conn0.watcher4"),
            Registry::default(),
            None,
        );

        let t = std::thread::spawn(move || {
            let req = server_read(&server);
            assert_eq!(req["to"], "server1.conn0.watcher4");
            assert_eq!(req["type"], "unwatchResources");
            assert_eq!(req["resourceTypes"], json!(["console-message"]));
            server_reply(&server, json!({"from": "server1.conn0.watcher4"}));
        });

        front
            .unwatch_resources(&mut transport, &["console-message"])
            .unwrap();
        t.join().unwrap();
    }

    #[test]
    fn unwatch_targets_sends_target_type_no_reply_expected() {
        // unwatchTargets is oneway — the server sends no reply.
        // We verify the request is sent correctly and that the call returns
        // immediately without waiting for a response.
        let (mut transport, server) = make_transport_pair();
        let front = WatcherFront::new(
            ActorId::from("server1.conn0.watcher4"),
            Registry::default(),
            None,
        );

        let t = std::thread::spawn(move || {
            let req = server_read(&server);
            assert_eq!(req["to"], "server1.conn0.watcher4");
            assert_eq!(req["type"], "unwatchTargets");
            assert_eq!(req["targetType"], "frame");
            // No reply sent — oneway method.
        });

        front.unwatch_targets(&mut transport, "frame").unwrap();
        t.join().unwrap();
    }
}
