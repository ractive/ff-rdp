use crate::actors::tab::TargetInfo;
use crate::error::ProtocolError;
use crate::registry::{Front, FrontKind, Registry};
use crate::specs::{NoArgs, call, descriptor as spec};
use crate::transport::RdpTransport;
use crate::types::ActorId;

/// A typed handle to a Firefox tab descriptor actor.
///
/// Descriptor actors expose `getTarget` and `getWatcher` — they are the
/// per-tab entry points returned by the root actor's `listTabs`.
///
/// Creating a `DescriptorFront` is O(1) and does not touch the network.
pub struct DescriptorFront {
    id: ActorId,
    registry: Registry,
}

impl DescriptorFront {
    /// Wrap an actor ID as a `DescriptorFront` and register it in the registry.
    pub fn new(id: ActorId, registry: Registry) -> Self {
        registry.register(id.clone(), FrontKind::Descriptor, None);
        Self { id, registry }
    }

    /// Call `getTarget` to obtain the WindowGlobalTarget and associated actor IDs.
    ///
    /// Returns a `TargetInfo` parsed from the typed `frame` field.
    pub fn get_target(&self, transport: &mut RdpTransport) -> Result<TargetInfo, ProtocolError> {
        let reply = call::<spec::GetTarget>(transport, &self.id, &NoArgs {})?;
        let frame = reply.frame.ok_or_else(|| {
            ProtocolError::InvalidPacket("getTarget response missing 'frame' object".into())
        })?;
        Ok(TargetInfo {
            actor: frame.actor,
            console_actor: frame.console_actor,
            thread_actor: frame.thread_actor,
            inspector_actor: frame.inspector_actor,
            screenshot_content_actor: frame.screenshot_content_actor,
            accessibility_actor: frame.accessibility_actor,
            responsive_actor: frame.responsive_actor,
            browsing_context_id: frame.browsing_context_id,
        })
    }

    /// Call `getWatcher` to obtain the watcher actor ID for this tab.
    pub fn get_watcher(&self, transport: &mut RdpTransport) -> Result<ActorId, ProtocolError> {
        let reply = call::<spec::GetWatcher>(transport, &self.id, &NoArgs {})?;
        if reply.actor.as_ref().is_empty() {
            return Err(ProtocolError::InvalidPacket(
                "getWatcher response missing 'actor' field".into(),
            ));
        }
        Ok(reply.actor)
    }
}

impl Front for DescriptorFront {
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
    fn get_target_parses_frame_into_target_info() {
        let (mut transport, server) = make_transport_pair();
        let front = DescriptorFront::new(
            ActorId::from("server1.conn3.tabDescriptor1"),
            Registry::default(),
        );

        let t = std::thread::spawn(move || {
            let req = server_read(&server);
            assert_eq!(req["type"], "getTarget");
            server_reply(
                &server,
                json!({
                    "from": "server1.conn3.tabDescriptor1",
                    "frame": {
                        "actor": "server1.conn3.child2/windowGlobalTarget2",
                        "consoleActor": "server1.conn3.child2/consoleActor3",
                        "browsingContextID": 55
                    }
                }),
            );
        });

        let info = front.get_target(&mut transport).unwrap();
        assert_eq!(
            info.actor.as_ref(),
            "server1.conn3.child2/windowGlobalTarget2"
        );
        assert_eq!(
            info.console_actor.as_ref(),
            "server1.conn3.child2/consoleActor3"
        );
        assert_eq!(info.browsing_context_id, Some(55));
        t.join().unwrap();
    }

    #[test]
    fn get_watcher_returns_actor_id() {
        let (mut transport, server) = make_transport_pair();
        let front = DescriptorFront::new(
            ActorId::from("server1.conn3.tabDescriptor1"),
            Registry::default(),
        );

        let t = std::thread::spawn(move || {
            let req = server_read(&server);
            assert_eq!(req["type"], "getWatcher");
            server_reply(
                &server,
                json!({
                    "from": "server1.conn3.tabDescriptor1",
                    "actor": "server1.conn3.watcher4"
                }),
            );
        });

        let actor_id = front.get_watcher(&mut transport).unwrap();
        assert_eq!(actor_id.as_ref(), "server1.conn3.watcher4");
        t.join().unwrap();
    }
}
