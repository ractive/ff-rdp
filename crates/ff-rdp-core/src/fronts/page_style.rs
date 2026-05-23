use crate::actors::page_style::{AppliedRule, BoxModelLayout, ComputedProperty, PageStyleActor};
use crate::error::ProtocolError;
use crate::registry::{Front, FrontKind, Registry};
use crate::specs::{call, page_style as spec};
use crate::transport::RdpTransport;
use crate::types::ActorId;

/// A typed handle to a Firefox `PageStyle` actor.
///
/// PageStyle actors provide `getApplied`, `getComputed`, `getBoxModel`, etc.
/// for CSS inspection of DOM nodes.
///
/// Creating a `PageStyleFront` is O(1) and does not touch the network.
pub struct PageStyleFront {
    id: ActorId,
    registry: Registry,
}

impl PageStyleFront {
    /// Wrap an actor ID as a `PageStyleFront` and register it in the registry.
    ///
    /// `target_root` should be the `WindowGlobalTarget` actor that owns this front.
    pub fn new(id: ActorId, registry: Registry, target_root: ActorId) -> Self {
        registry.register(id.clone(), FrontKind::PageStyle, Some(target_root));
        Self { id, registry }
    }

    /// Get computed styles for a DOM node.
    ///
    /// Delegates to `PageStyleActor::get_computed` for the complex response parsing.
    pub fn get_computed(
        &self,
        transport: &mut RdpTransport,
        node_actor: &ActorId,
    ) -> Result<Vec<ComputedProperty>, ProtocolError> {
        PageStyleActor::get_computed(transport, &self.id, node_actor)
    }

    /// Get applied CSS rules for a DOM node.
    ///
    /// Delegates to `PageStyleActor::get_applied` for the complex response parsing.
    pub fn get_applied(
        &self,
        transport: &mut RdpTransport,
        node_actor: &ActorId,
    ) -> Result<Vec<AppliedRule>, ProtocolError> {
        PageStyleActor::get_applied(transport, &self.id, node_actor)
    }

    /// Get the box model layout for a DOM node.
    ///
    /// Delegates to `PageStyleActor::get_layout` for the complex response parsing.
    pub fn get_layout(
        &self,
        transport: &mut RdpTransport,
        node_actor: &ActorId,
    ) -> Result<BoxModelLayout, ProtocolError> {
        PageStyleActor::get_layout(transport, &self.id, node_actor)
    }

    /// Get computed styles as typed spec response (raw computed map).
    pub fn get_computed_raw(
        &self,
        transport: &mut RdpTransport,
        node_actor: &ActorId,
    ) -> Result<spec::response::GetComputed, ProtocolError> {
        let args = spec::request::GetComputed {
            node: node_actor.as_ref().to_string(),
            mark_matched: true,
            filter: "user".to_string(),
        };
        call::<spec::GetComputed>(transport, &self.id, &args)
    }
}

impl Front for PageStyleFront {
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
    fn get_computed_raw_sends_correct_request() {
        let (mut transport, server) = make_transport_pair();
        let front = PageStyleFront::new(
            ActorId::from("server1.conn0.child1/pageStyleActor1"),
            Registry::default(),
            ActorId::from("server1.conn0.child1/windowGlobalTarget1"),
        );
        let node_actor = ActorId::from("server1.conn0.child1/domNode1");

        let t = std::thread::spawn(move || {
            let req = server_read(&server);
            assert_eq!(req["type"], "getComputed");
            assert_eq!(req["node"], "server1.conn0.child1/domNode1");
            assert_eq!(req["markMatched"], true);
            assert_eq!(req["filter"], "user");
            server_reply(
                &server,
                json!({
                    "from": "server1.conn0.child1/pageStyleActor1",
                    "computed": {"color": {"value": "rgb(0,0,0)", "priority": ""}}
                }),
            );
        });

        let reply = front.get_computed_raw(&mut transport, &node_actor).unwrap();
        assert!(reply.computed.get("color").is_some());
        t.join().unwrap();
    }
}
