use crate::actors::tab::TabInfo;
use crate::error::ProtocolError;
use crate::registry::{Front, FrontKind, Registry};
use crate::specs::{NoArgs, call, root as spec};
use crate::transport::RdpTransport;
use crate::types::ActorId;

/// A typed handle to the Firefox RDP root actor.
///
/// The root actor is the entry point for all RDP sessions — it exposes
/// `listTabs`, `listProcesses`, and other top-level discovery methods.
///
/// Creating a `RootFront` is O(1) and does not touch the network.
pub struct RootFront {
    id: ActorId,
    registry: Registry,
}

impl RootFront {
    /// Wrap an actor ID as a `RootFront` and register it in the registry.
    pub fn new(id: ActorId, registry: Registry) -> Self {
        registry.register(id.clone(), FrontKind::Root, None);
        Self { id, registry }
    }

    /// List all open browser tabs.
    ///
    /// Note: Firefox may interleave `tabListChanged` push events between the
    /// request and the reply.  For full push-event filtering, use
    /// [`crate::actors::root::RootActor::list_tabs`] directly.
    /// This typed method uses the generic `call` helper and returns only the
    /// first reply from the root actor.
    pub fn list_tabs(&self, transport: &mut RdpTransport) -> Result<Vec<TabInfo>, ProtocolError> {
        let reply = call::<spec::ListTabs>(transport, &self.id, &NoArgs {})?;
        Ok(reply.tabs)
    }

    /// Get root actor metadata (service actor IDs like `screenshotActor`, etc.).
    pub fn get_root(
        &self,
        transport: &mut RdpTransport,
    ) -> Result<spec::response::GetRoot, ProtocolError> {
        call::<spec::GetRoot>(transport, &self.id, &NoArgs {})
    }

    /// List all browser processes.
    pub fn list_processes(
        &self,
        transport: &mut RdpTransport,
    ) -> Result<spec::response::ListProcesses, ProtocolError> {
        call::<spec::ListProcesses>(transport, &self.id, &NoArgs {})
    }
}

impl Front for RootFront {
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
    fn list_tabs_returns_typed_tab_info() {
        let (mut transport, server) = make_transport_pair();
        let front = RootFront::new(ActorId::from("root"), Registry::default());

        let t = std::thread::spawn(move || {
            let req = server_read(&server);
            assert_eq!(req["type"], "listTabs");
            server_reply(
                &server,
                json!({
                    "from": "root",
                    "tabs": [{"actor": "server1.conn0.tabDescriptor1", "title": "Test", "url": "https://test.com", "selected": true}]
                }),
            );
        });

        let tabs = front.list_tabs(&mut transport).unwrap();
        assert_eq!(tabs.len(), 1);
        assert_eq!(tabs[0].title, "Test");
        t.join().unwrap();
    }

    #[test]
    fn get_root_returns_service_actors() {
        let (mut transport, server) = make_transport_pair();
        let front = RootFront::new(ActorId::from("root"), Registry::default());

        let t = std::thread::spawn(move || {
            let _req = server_read(&server);
            server_reply(
                &server,
                json!({
                    "from": "root",
                    "screenshotActor": "server1.conn0.screenshotActor7"
                }),
            );
        });

        let root = front.get_root(&mut transport).unwrap();
        assert_eq!(
            root.screenshot_actor
                .as_ref()
                .map(std::convert::AsRef::as_ref),
            Some("server1.conn0.screenshotActor7")
        );
        t.join().unwrap();
    }
}
