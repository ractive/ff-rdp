use crate::actors::dom_walker::parse_dom_node;
use crate::error::ProtocolError;
use crate::registry::{Front, FrontKind, Registry};
use crate::specs::{NoArgs, call, walker as spec};
use crate::transport::RdpTransport;
use crate::types::ActorId;

/// A typed handle to a Firefox `DOMWalker` actor.
///
/// Walker actors traverse the DOM tree for a target and expose `getRootNode`,
/// `children`, `node`, etc.
///
/// Creating a `WalkerFront` is O(1) and does not touch the network.
pub struct WalkerFront {
    id: ActorId,
    registry: Registry,
}

impl WalkerFront {
    /// Wrap an actor ID as a `WalkerFront` and register it in the registry.
    ///
    /// `target_root` should be the `WindowGlobalTarget` actor that owns this front.
    pub fn new(id: ActorId, registry: Registry, target_root: ActorId) -> Self {
        registry.register(id.clone(), FrontKind::Walker, Some(target_root));
        Self { id, registry }
    }

    /// Get the document element node (the `<html>` element).
    pub fn document_element(
        &self,
        transport: &mut RdpTransport,
    ) -> Result<Option<spec::DomNode>, ProtocolError> {
        let reply = call::<spec::DocumentElement>(transport, &self.id, &NoArgs {})?;
        match reply.node {
            Some(node) => parse_dom_node(transport, &node),
            None => Ok(None),
        }
    }

    /// Find a single DOM node matching a CSS selector.
    pub fn query_selector(
        &self,
        transport: &mut RdpTransport,
        node_actor: &ActorId,
        selector: &str,
    ) -> Result<Option<spec::DomNode>, ProtocolError> {
        let args = spec::request::QuerySelector {
            node: node_actor.as_ref().to_string(),
            selector: selector.to_string(),
        };
        let reply = call::<spec::QuerySelector>(transport, &self.id, &args)?;
        match reply.node {
            Some(node) => parse_dom_node(transport, &node),
            None => Ok(None),
        }
    }

    /// Find all DOM nodes matching a CSS selector (returns a node list actor reference).
    pub fn query_selector_all(
        &self,
        transport: &mut RdpTransport,
        node_actor: &ActorId,
        selector: &str,
    ) -> Result<spec::response::QuerySelectorAll, ProtocolError> {
        let args = spec::request::QuerySelectorAll {
            node: node_actor.as_ref().to_string(),
            selector: selector.to_string(),
        };
        call::<spec::QuerySelectorAll>(transport, &self.id, &args)
    }
}

impl Front for WalkerFront {
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
    fn document_element_sends_correct_request() {
        let (mut transport, server) = make_transport_pair();
        let front = WalkerFront::new(
            ActorId::from("server1.conn0.child1/domWalker1"),
            Registry::default(),
            ActorId::from("server1.conn0.child1/windowGlobalTarget1"),
        );

        let t = std::thread::spawn(move || {
            let req = server_read(&server);
            assert_eq!(req["type"], "documentElement");
            assert_eq!(req["to"], "server1.conn0.child1/domWalker1");
            server_reply(
                &server,
                json!({
                    "from": "server1.conn0.child1/domWalker1",
                    "node": {
                        "actor": "server1.conn0.child1/domNode1",
                        "nodeType": 1,
                        "nodeName": "HTML",
                        "attrs": []
                    }
                }),
            );
        });

        let node = front.document_element(&mut transport).unwrap();
        assert!(node.is_some());
        assert_eq!(node.unwrap().node_name, "HTML");
        t.join().unwrap();
    }

    #[test]
    fn query_selector_sends_node_and_selector() {
        let (mut transport, server) = make_transport_pair();
        let front = WalkerFront::new(
            ActorId::from("server1.conn0.child1/domWalker1"),
            Registry::default(),
            ActorId::from("server1.conn0.child1/windowGlobalTarget1"),
        );
        let node_actor = ActorId::from("server1.conn0.child1/domNode1");

        let t = std::thread::spawn(move || {
            let req = server_read(&server);
            assert_eq!(req["type"], "querySelector");
            assert_eq!(req["node"], "server1.conn0.child1/domNode1");
            assert_eq!(req["selector"], "h1");
            server_reply(&server, json!({"from": "server1.conn0.child1/domWalker1"}));
        });

        let result = front
            .query_selector(&mut transport, &node_actor, "h1")
            .unwrap();
        assert!(result.is_none()); // no node in response → None
        t.join().unwrap();
    }
}
