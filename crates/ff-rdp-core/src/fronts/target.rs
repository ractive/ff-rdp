use crate::error::ProtocolError;
use crate::registry::{Front, FrontKind, Registry};
use crate::specs::{NoArgs, call, target as spec};
use crate::transport::RdpTransport;
use crate::types::ActorId;

/// A typed handle to a Firefox `WindowGlobalTarget` actor.
///
/// Target actors are scoped to a browsing context (frame).  They expose
/// `navigate`, `reload`, and are the root under which console, inspector,
/// and other per-target actors live.
///
/// Creating a `TargetFront` is O(1) and does not touch the network.
pub struct TargetFront {
    id: ActorId,
    registry: Registry,
}

impl TargetFront {
    /// Wrap an actor ID as a `TargetFront` and register it in the registry.
    ///
    /// The target itself has no owning root (it *is* the root for its subtree).
    pub fn new(id: ActorId, registry: Registry) -> Self {
        registry.register(id.clone(), FrontKind::Target, None);
        Self { id, registry }
    }

    /// Navigate to the given URL.
    pub fn navigate_to(
        &self,
        transport: &mut RdpTransport,
        url: &str,
    ) -> Result<(), ProtocolError> {
        let args = spec::request::NavigateTo {
            url: url.to_string(),
        };
        call::<spec::NavigateTo>(transport, &self.id, &args)?;
        Ok(())
    }

    /// Reload the current page.
    pub fn reload(&self, transport: &mut RdpTransport) -> Result<(), ProtocolError> {
        call::<spec::Reload>(transport, &self.id, &NoArgs {})?;
        Ok(())
    }

    /// Go back in browser history.
    pub fn go_back(&self, transport: &mut RdpTransport) -> Result<(), ProtocolError> {
        call::<spec::GoBack>(transport, &self.id, &NoArgs {})?;
        Ok(())
    }

    /// Go forward in browser history.
    pub fn go_forward(&self, transport: &mut RdpTransport) -> Result<(), ProtocolError> {
        call::<spec::GoForward>(transport, &self.id, &NoArgs {})?;
        Ok(())
    }
}

impl Front for TargetFront {
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
    fn navigate_to_sends_url() {
        let (mut transport, server) = make_transport_pair();
        let front = TargetFront::new(
            ActorId::from("server1.conn0.child1/windowGlobalTarget1"),
            Registry::default(),
        );

        let t = std::thread::spawn(move || {
            let req = server_read(&server);
            assert_eq!(req["type"], "navigateTo");
            assert_eq!(req["url"], "https://example.com");
            server_reply(
                &server,
                json!({"from": "server1.conn0.child1/windowGlobalTarget1"}),
            );
        });

        front
            .navigate_to(&mut transport, "https://example.com")
            .unwrap();
        t.join().unwrap();
    }

    #[test]
    fn reload_sends_reload_request() {
        let (mut transport, server) = make_transport_pair();
        let front = TargetFront::new(
            ActorId::from("server1.conn0.child1/windowGlobalTarget1"),
            Registry::default(),
        );

        let t = std::thread::spawn(move || {
            let req = server_read(&server);
            assert_eq!(req["type"], "reload");
            server_reply(
                &server,
                json!({"from": "server1.conn0.child1/windowGlobalTarget1"}),
            );
        });

        front.reload(&mut transport).unwrap();
        t.join().unwrap();
    }
}
