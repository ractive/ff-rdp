use serde_json::Value;

use crate::actors::root::RootActor;
use crate::actors::tab::TabInfo;
use crate::error::{ActorErrorKind, ProtocolError};
use crate::fronts::ProcessDescriptorFront;
use crate::registry::{Front, FrontKind, Registry};
use crate::specs::root as spec;
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
    /// Delegates to [`RootActor::list_tabs`] which already filters
    /// `tabListChanged`-style push events (packets from `"root"` that carry a
    /// `type` field) before returning the authoritative reply.
    pub fn list_tabs(&self, transport: &mut RdpTransport) -> Result<Vec<TabInfo>, ProtocolError> {
        RootActor::list_tabs(transport)
    }

    /// Get root actor metadata (service actor IDs like `screenshotActor`, etc.).
    ///
    /// Sends `getRoot` and reads packets, skipping any push events from the root
    /// actor (packets with a `type` field) until the authoritative reply arrives.
    pub fn get_root(
        &self,
        transport: &mut RdpTransport,
    ) -> Result<spec::response::GetRoot, ProtocolError> {
        self.filtered_call(transport, "getRoot", |v| {
            serde_json::from_value(v)
                .map_err(|e| ProtocolError::InvalidPacket(format!("decode getRoot: {e}")))
        })
    }

    /// List all browser processes.
    ///
    /// Sends `listProcesses` and reads packets, skipping any push events from
    /// the root actor (packets with a `type` field) until the reply arrives.
    pub fn list_processes(
        &self,
        transport: &mut RdpTransport,
    ) -> Result<spec::response::ListProcesses, ProtocolError> {
        self.filtered_call(transport, "listProcesses", |v| {
            serde_json::from_value(v)
                .map_err(|e| ProtocolError::InvalidPacket(format!("decode listProcesses: {e}")))
        })
    }

    /// Get a process descriptor front for the browser process with the given OS `id`.
    ///
    /// Pass `id = 0` to get the browser parent process (main process), which
    /// hosts chrome-privileged APIs including the parent-process console actor.
    /// The returned [`ProcessDescriptorFront`] can then call `get_target()` to
    /// obtain the chrome-privileged `consoleActor`.
    ///
    /// Sends `getProcess` and skips any push events until the authoritative reply.
    pub fn get_process(
        &self,
        transport: &mut RdpTransport,
        id: u32,
    ) -> Result<ProcessDescriptorFront, ProtocolError> {
        use serde_json::json;

        let actor_id = self.filtered_call_with_args(
            transport,
            "getProcess",
            Some(json!({ "id": id })),
            |v| {
                let actor_str = v
                    .get("processDescriptor")
                    .and_then(|pd| pd.get("actor"))
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        ProtocolError::InvalidPacket(
                            "getProcess response missing 'processDescriptor.actor' field".into(),
                        )
                    })?;
                Ok(ActorId::from(actor_str))
            },
        )?;
        Ok(ProcessDescriptorFront::new(
            actor_id,
            self.registry.clone(),
            Some(self.id.clone()),
        ))
    }

    /// Send `method` to the root actor and loop, skipping push events (packets
    /// from root that carry a `type` field), then pass the reply through `parse`.
    fn filtered_call<T>(
        &self,
        transport: &mut RdpTransport,
        method: &str,
        parse: impl FnOnce(Value) -> Result<T, ProtocolError>,
    ) -> Result<T, ProtocolError> {
        self.filtered_call_with_args(transport, method, None, parse)
    }

    /// Like [`filtered_call`] but merges extra JSON fields into the request.
    fn filtered_call_with_args<T>(
        &self,
        transport: &mut RdpTransport,
        method: &str,
        extra: Option<Value>,
        parse: impl FnOnce(Value) -> Result<T, ProtocolError>,
    ) -> Result<T, ProtocolError> {
        use serde_json::json;

        let mut request = json!({"to": self.id.as_ref(), "type": method});
        if let Some(extra_val) = extra
            && let (Some(req_obj), Some(extra_obj)) =
                (request.as_object_mut(), extra_val.as_object())
        {
            for (k, v) in extra_obj {
                req_obj.insert(k.clone(), v.clone());
            }
        }
        transport.send(&request)?;

        let response = loop {
            let msg = transport.recv()?;
            let from = msg.get("from").and_then(Value::as_str).unwrap_or_default();
            if from == self.id.as_ref() {
                if msg.get("type").is_some() {
                    // Push event — skip it.
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
                break msg;
            }
        };

        parse(response)
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
    fn list_tabs_skips_tab_list_changed_push_event() {
        let (mut transport, server) = make_transport_pair();
        let front = RootFront::new(ActorId::from("root"), Registry::default());

        let t = std::thread::spawn(move || {
            let _req = server_read(&server);
            // Push event — has a `type` field, should be skipped.
            server_reply(&server, json!({"from": "root", "type": "tabListChanged"}));
            // Real reply.
            server_reply(
                &server,
                json!({
                    "from": "root",
                    "tabs": [{"actor": "server1.conn0.tabDescriptor1", "title": "After", "url": "https://after.com", "selected": false}]
                }),
            );
        });

        let tabs = front.list_tabs(&mut transport).unwrap();
        assert_eq!(tabs.len(), 1);
        assert_eq!(tabs[0].title, "After");
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

    #[test]
    fn get_root_skips_push_event() {
        let (mut transport, server) = make_transport_pair();
        let front = RootFront::new(ActorId::from("root"), Registry::default());

        let t = std::thread::spawn(move || {
            let _req = server_read(&server);
            // Push event before the real reply.
            server_reply(&server, json!({"from": "root", "type": "tabListChanged"}));
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

    #[test]
    fn list_processes_skips_push_event() {
        let (mut transport, server) = make_transport_pair();
        let front = RootFront::new(ActorId::from("root"), Registry::default());

        let t = std::thread::spawn(move || {
            let _req = server_read(&server);
            // Push event before the real reply.
            server_reply(&server, json!({"from": "root", "type": "tabListChanged"}));
            server_reply(
                &server,
                json!({
                    "from": "root",
                    "processes": [
                        {"actor": "server1.conn0.processDescriptor1", "isParent": true}
                    ]
                }),
            );
        });

        let procs = front.list_processes(&mut transport).unwrap();
        assert_eq!(procs.processes.len(), 1);
        assert!(procs.processes[0].is_parent);
        t.join().unwrap();
    }
}
