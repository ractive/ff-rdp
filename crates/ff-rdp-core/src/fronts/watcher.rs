use crate::error::ProtocolError;
use crate::registry::{Front, FrontKind, Registry};
use crate::specs::{call, watcher as spec};
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
    pub fn watch_resources(
        &self,
        transport: &mut RdpTransport,
        types: &[&str],
    ) -> Result<(), ProtocolError> {
        let args = spec::request::WatchResources {
            resource_types: types.iter().map(|s| (*s).to_string()).collect(),
        };
        call::<spec::WatchResources>(transport, &self.id, &args)?;
        Ok(())
    }

    /// Unsubscribe from one or more resource types.
    pub fn unwatch_resources(
        &self,
        transport: &mut RdpTransport,
        types: &[&str],
    ) -> Result<(), ProtocolError> {
        let args = spec::request::UnwatchResources {
            resource_types: types.iter().map(|s| (*s).to_string()).collect(),
        };
        call::<spec::UnwatchResources>(transport, &self.id, &args)?;
        Ok(())
    }

    /// Subscribe to target events of the given type.
    ///
    /// Target types: `"frame"`, `"worker"`, `"process"`, etc.
    pub fn watch_targets(
        &self,
        transport: &mut RdpTransport,
        target_type: &str,
    ) -> Result<(), ProtocolError> {
        let args = spec::request::WatchTargets {
            target_type: target_type.to_string(),
        };
        call::<spec::WatchTargets>(transport, &self.id, &args)?;
        Ok(())
    }

    /// Unsubscribe from target events of the given type.
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
}
