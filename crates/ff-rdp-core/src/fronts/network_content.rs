use crate::error::ProtocolError;
use crate::registry::{Front, FrontKind, Registry};
use crate::specs::{NoArgs, call, network_event as spec};
use crate::transport::RdpTransport;
use crate::types::ActorId;

/// A typed handle to a Firefox `NetworkEvent` (network content) actor.
///
/// NetworkContent actors provide `getRequestHeaders`, `getResponseContent`,
/// etc. for individual network requests captured by the watcher.
///
/// Creating a `NetworkContentFront` is O(1) and does not touch the network.
pub struct NetworkContentFront {
    id: ActorId,
    registry: Registry,
}

impl NetworkContentFront {
    /// Wrap an actor ID as a `NetworkContentFront` and register it in the registry.
    ///
    /// `target_root` should be the `WindowGlobalTarget` actor that owns this front.
    pub fn new(id: ActorId, registry: Registry, target_root: ActorId) -> Self {
        registry.register(id.clone(), FrontKind::NetworkContent, Some(target_root));
        Self { id, registry }
    }

    /// Fetch the request headers for this network event.
    pub fn get_request_headers(
        &self,
        transport: &mut RdpTransport,
    ) -> Result<spec::response::GetRequestHeaders, ProtocolError> {
        call::<spec::GetRequestHeaders>(transport, &self.id, &NoArgs {})
    }

    /// Fetch the response headers for this network event.
    pub fn get_response_headers(
        &self,
        transport: &mut RdpTransport,
    ) -> Result<spec::response::GetResponseHeaders, ProtocolError> {
        call::<spec::GetResponseHeaders>(transport, &self.id, &NoArgs {})
    }

    /// Fetch the response body content for this network event.
    pub fn get_response_content(
        &self,
        transport: &mut RdpTransport,
    ) -> Result<spec::response::GetResponseContent, ProtocolError> {
        call::<spec::GetResponseContent>(transport, &self.id, &NoArgs {})
    }
}

impl Front for NetworkContentFront {
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
    fn get_request_headers_sends_correct_request() {
        let (mut transport, server) = make_transport_pair();
        let front = NetworkContentFront::new(
            ActorId::from("server1.conn0.netEvent6"),
            Registry::default(),
            ActorId::from("server1.conn0.child1/windowGlobalTarget1"),
        );

        let t = std::thread::spawn(move || {
            let req = server_read(&server);
            assert_eq!(req["type"], "getRequestHeaders");
            assert_eq!(req["to"], "server1.conn0.netEvent6");
            server_reply(
                &server,
                json!({
                    "from": "server1.conn0.netEvent6",
                    "headers": [{"name": "Accept", "value": "text/html"}],
                    "headersSize": 50
                }),
            );
        });

        let reply = front.get_request_headers(&mut transport).unwrap();
        assert_eq!(reply.headers.len(), 1);
        assert_eq!(reply.headers[0].name, "Accept");
        t.join().unwrap();
    }

    #[test]
    fn get_response_headers_sends_correct_request() {
        let (mut transport, server) = make_transport_pair();
        let front = NetworkContentFront::new(
            ActorId::from("server1.conn0.netEvent6"),
            Registry::default(),
            ActorId::from("server1.conn0.child1/windowGlobalTarget1"),
        );

        let t = std::thread::spawn(move || {
            let req = server_read(&server);
            assert_eq!(req["type"], "getResponseHeaders");
            server_reply(
                &server,
                json!({
                    "from": "server1.conn0.netEvent6",
                    "headers": [{"name": "Content-Type", "value": "text/html"}],
                    "headersSize": 48
                }),
            );
        });

        let reply = front.get_response_headers(&mut transport).unwrap();
        assert_eq!(reply.headers[0].name, "Content-Type");
        t.join().unwrap();
    }
}
