use crate::error::ProtocolError;
use crate::registry::{Front, FrontKind, Registry};
use crate::specs::{NoArgs, call, descriptor as spec};
use crate::transport::RdpTransport;
use crate::types::ActorId;

/// The console actor ID and target actor ID for a browser process.
///
/// Returned by [`ProcessDescriptorFront::get_target`].  The `console_actor`
/// is chrome-privileged; use it with [`crate::WebConsoleActor::evaluate_js_async`]
/// (no `chromeContext` flag needed) to execute chrome-context JavaScript.
#[derive(Debug, Clone)]
pub struct ProcessTarget {
    pub actor: ActorId,
    pub console_actor: ActorId,
}

/// A typed handle to a Firefox process descriptor actor.
///
/// Process descriptor actors are returned by the root actor's `getProcess(id)`
/// method.  They expose `getTarget`, which returns the process-level target
/// form containing a chrome-privileged `consoleActor`.
///
/// Chrome-context JavaScript evaluation routes through this actor rather than
/// using the deprecated `chromeContext: true` flag on the tab console actor.
///
/// Creating a `ProcessDescriptorFront` is O(1) and does not touch the network.
pub struct ProcessDescriptorFront {
    id: ActorId,
    registry: Registry,
}

impl ProcessDescriptorFront {
    /// Wrap an actor ID as a `ProcessDescriptorFront` and register it in the registry.
    pub fn new(id: ActorId, registry: Registry, parent: Option<ActorId>) -> Self {
        registry.register(id.clone(), FrontKind::Descriptor, parent);
        Self { id, registry }
    }

    /// Call `getTarget` to obtain the process target and its chrome-privileged console actor.
    pub fn get_target(&self, transport: &mut RdpTransport) -> Result<ProcessTarget, ProtocolError> {
        let reply = call::<spec::GetProcessTarget>(transport, &self.id, &NoArgs {})?;
        let form = reply.process_descriptor;
        if form.actor.as_ref().is_empty() {
            return Err(ProtocolError::InvalidPacket(
                "getTarget response missing process actor".into(),
            ));
        }
        if form.console_actor.as_ref().is_empty() {
            return Err(ProtocolError::InvalidPacket(
                "getTarget response missing consoleActor for process target".into(),
            ));
        }
        Ok(ProcessTarget {
            actor: form.actor,
            console_actor: form.console_actor,
        })
    }
}

impl Front for ProcessDescriptorFront {
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
    fn get_target_parses_console_actor_from_process_descriptor() {
        let (mut transport, server) = make_transport_pair();
        let front = ProcessDescriptorFront::new(
            ActorId::from("server1.conn0.processDescriptor1"),
            Registry::default(),
            None,
        );

        let t = std::thread::spawn(move || {
            let req = server_read(&server);
            assert_eq!(req["type"], "getTarget");
            assert_eq!(req["to"], "server1.conn0.processDescriptor1");
            server_reply(
                &server,
                json!({
                    "from": "server1.conn0.processDescriptor1",
                    "processDescriptor": {
                        "actor": "server1.conn0/parentProcessTarget",
                        "consoleActor": "server1.conn0/consoleActor99",
                    }
                }),
            );
        });

        let target = front.get_target(&mut transport).unwrap();
        assert_eq!(target.actor.as_ref(), "server1.conn0/parentProcessTarget");
        assert_eq!(
            target.console_actor.as_ref(),
            "server1.conn0/consoleActor99"
        );
        t.join().unwrap();
    }

    #[test]
    fn get_target_returns_error_when_console_actor_missing() {
        let (mut transport, server) = make_transport_pair();
        let front = ProcessDescriptorFront::new(
            ActorId::from("server1.conn0.processDescriptor1"),
            Registry::default(),
            None,
        );

        let t = std::thread::spawn(move || {
            let _req = server_read(&server);
            server_reply(
                &server,
                json!({
                    "from": "server1.conn0.processDescriptor1",
                    "processDescriptor": {
                        "actor": "server1.conn0/parentProcessTarget",
                        "consoleActor": "",
                    }
                }),
            );
        });

        let err = front.get_target(&mut transport).unwrap_err();
        assert!(
            err.to_string().contains("consoleActor"),
            "error should mention missing consoleActor: {err}"
        );
        t.join().unwrap();
    }
}
