use crate::actors::console::{EvalResult, WebConsoleActor};
use crate::error::ProtocolError;
use crate::registry::{Front, FrontKind, Registry};
use crate::specs::{call, console as spec};
use crate::transport::RdpTransport;
use crate::types::ActorId;

/// A typed handle to a Firefox `WebConsole` actor.
///
/// Console actors are scoped to a target and expose `evaluateJSAsync`,
/// `startListeners`, `getCachedMessages`, etc.
///
/// Creating a `ConsoleFront` is O(1) and does not touch the network.
pub struct ConsoleFront {
    id: ActorId,
    registry: Registry,
}

impl ConsoleFront {
    /// Wrap an actor ID as a `ConsoleFront` and register it in the registry.
    ///
    /// `target_root` should be the `WindowGlobalTarget` actor that owns this console.
    pub fn new(id: ActorId, registry: Registry, target_root: ActorId) -> Self {
        registry.register(id.clone(), FrontKind::Console, Some(target_root));
        Self { id, registry }
    }

    /// Start listeners for console events.
    ///
    /// Valid listener types: `"PageError"`, `"ConsoleAPI"`.
    pub fn start_listeners(
        &self,
        transport: &mut RdpTransport,
        listeners: &[&str],
    ) -> Result<spec::response::StartListeners, ProtocolError> {
        let args = spec::request::StartListeners {
            listeners: listeners.iter().map(|s| (*s).to_string()).collect(),
        };
        call::<spec::StartListeners>(transport, &self.id, &args)
    }

    /// Stop listeners for console events.
    pub fn stop_listeners(
        &self,
        transport: &mut RdpTransport,
        listeners: &[&str],
    ) -> Result<spec::response::StopListeners, ProtocolError> {
        let args = spec::request::StopListeners {
            listeners: listeners.iter().map(|s| (*s).to_string()).collect(),
        };
        call::<spec::StopListeners>(transport, &self.id, &args)
    }

    /// Retrieve cached console messages.
    ///
    /// Message types: `"PageError"`, `"ConsoleAPI"`.
    pub fn get_cached_messages(
        &self,
        transport: &mut RdpTransport,
        message_types: &[&str],
    ) -> Result<spec::response::GetCachedMessages, ProtocolError> {
        let args = spec::request::GetCachedMessages {
            message_types: message_types.iter().map(|s| (*s).to_string()).collect(),
        };
        call::<spec::GetCachedMessages>(transport, &self.id, &args)
    }

    /// Evaluate a JavaScript expression asynchronously.
    ///
    /// Uses the two-packet protocol (immediate ack → async `evaluationResult` event)
    /// which cannot be handled by the generic `call` helper.  Delegates to the
    /// existing `WebConsoleActor::evaluate_js_async` implementation.
    pub fn evaluate_js_async(
        &self,
        transport: &mut RdpTransport,
        text: &str,
    ) -> Result<EvalResult, ProtocolError> {
        WebConsoleActor::evaluate_js_async(transport, &self.id, text)
    }
}

impl Front for ConsoleFront {
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
    fn start_listeners_sends_listeners_array() {
        let (mut transport, server) = make_transport_pair();
        let front = ConsoleFront::new(
            ActorId::from("server1.conn0.child1/consoleActor1"),
            Registry::default(),
            ActorId::from("server1.conn0.child1/windowGlobalTarget1"),
        );

        let t = std::thread::spawn(move || {
            let req = server_read(&server);
            assert_eq!(req["type"], "startListeners");
            assert_eq!(req["listeners"], json!(["PageError", "ConsoleAPI"]));
            // Firefox returns `startedListeners`, not `listeners`.
            server_reply(
                &server,
                json!({
                    "from": "server1.conn0.child1/consoleActor1",
                    "startedListeners": ["PageError", "ConsoleAPI"]
                }),
            );
        });

        let reply = front
            .start_listeners(&mut transport, &["PageError", "ConsoleAPI"])
            .unwrap();
        assert_eq!(reply.listeners.len(), 2);
        t.join().unwrap();
    }

    #[test]
    fn get_cached_messages_sends_message_types() {
        let (mut transport, server) = make_transport_pair();
        let front = ConsoleFront::new(
            ActorId::from("server1.conn0.child1/consoleActor1"),
            Registry::default(),
            ActorId::from("server1.conn0.child1/windowGlobalTarget1"),
        );

        let t = std::thread::spawn(move || {
            let req = server_read(&server);
            assert_eq!(req["type"], "getCachedMessages");
            assert_eq!(req["messageTypes"], json!(["PageError"]));
            server_reply(
                &server,
                json!({
                    "from": "server1.conn0.child1/consoleActor1",
                    "messages": []
                }),
            );
        });

        let reply = front
            .get_cached_messages(&mut transport, &["PageError"])
            .unwrap();
        assert!(reply.messages.is_empty());
        t.join().unwrap();
    }
}
