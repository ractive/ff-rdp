use crate::actor::actor_request;
use crate::error::ProtocolError;
use crate::registry::{Front, FrontKind, Registry};
use crate::specs::{NoArgs, call, target as spec};
use crate::transport::RdpTransport;
use crate::types::ActorId;
use serde_json::json;

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
    ///
    /// Pass `force = true` to bypass the HTTP cache (sends the Firefox
    /// `{options: {force: true}}` request shape, equivalent to a hard
    /// reload in the browser UI).
    pub fn reload(&self, transport: &mut RdpTransport, force: bool) -> Result<(), ProtocolError> {
        if force {
            // The typed spec (`spec::Reload`) carries `NoArgs`, so it can't
            // attach the Firefox `options.force` request field.  Route the
            // extra param through `actor_request`, which sends the packet and
            // reads the reply via `recv_reply_from` — the matched-reply path
            // that routes any interleaved push event (e.g. a `tabNavigated`
            // fired by the reload itself) to the event sink instead of
            // mis-consuming it as this call's reply.  (Previously this used the
            // blind `transport.request` — send + one unmatched recv — which
            // desynced the actor's reply stream when a push arrived first.)
            let params = json!({ "options": { "force": true } });
            actor_request(transport, self.id.as_ref(), "reload", Some(&params))?;
            Ok(())
        } else {
            call::<spec::Reload>(transport, &self.id, &NoArgs {})?;
            Ok(())
        }
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

        front.reload(&mut transport, false).unwrap();
        t.join().unwrap();
    }

    /// iter-102 Theme B: `reload(force = true)` sends the Firefox
    /// `{options:{force:true}}` request shape and routes through the matched
    /// `actor_request` reply path.
    #[test]
    fn reload_force_sends_options_force_and_matches_reply() {
        let (mut transport, server) = make_transport_pair();
        let front = TargetFront::new(
            ActorId::from("server1.conn0.child1/windowGlobalTarget1"),
            Registry::default(),
        );

        let t = std::thread::spawn(move || {
            let req = server_read(&server);
            assert_eq!(req["type"], "reload");
            assert_eq!(req["options"]["force"], true);
            server_reply(
                &server,
                json!({"from": "server1.conn0.child1/windowGlobalTarget1"}),
            );
        });

        front.reload(&mut transport, true).unwrap();
        t.join().unwrap();
    }

    /// iter-102 Theme B (AC `live_reload_force_with_watched_resources`, unit
    /// analogue): a `tabNavigated` push event from the target actor arrives
    /// *before* the reload reply.  The matched `recv_reply_from` path must skip
    /// the push (routing it to the event sink) and consume the correct reply,
    /// so an immediately-following request on the same actor still gets *its*
    /// own reply — no stream desync.  The old blind `transport.request` would
    /// have consumed the `tabNavigated` push as the reply and left the real
    /// reply queued, desyncing the next call.
    #[test]
    fn reload_force_tolerates_tab_navigated_push_before_reply() {
        use std::sync::mpsc::channel;

        const TARGET: &str = "server1.conn0.child1/windowGlobalTarget1";

        let (mut transport, server) = make_transport_pair();
        // Route any interleaved push events off the reply path.
        let (tx, rx) = channel();
        transport.set_event_sink(Some(tx));

        let front = TargetFront::new(ActorId::from(TARGET), Registry::default());

        let t = std::thread::spawn(move || {
            // Read the reload request.
            let req = server_read(&server);
            assert_eq!(req["type"], "reload");
            assert_eq!(req["options"]["force"], true);
            // Push a same-actor typed event BEFORE the reply (the reload's own
            // tabNavigated — its most likely interleaving).
            server_reply(&server, json!({"from": TARGET, "type": "tabNavigated"}));
            // Then the actual reload reply (no `type`).
            server_reply(&server, json!({"from": TARGET}));
            // A follow-up request must receive its own distinct reply.
            let req2 = server_read(&server);
            assert_eq!(req2["type"], "navigateTo");
            server_reply(&server, json!({"from": TARGET}));
        });

        // reload must return Ok despite the interleaved push.
        front.reload(&mut transport, true).unwrap();

        // The push must have been routed to the event sink, not consumed as the
        // reply.
        let event = rx
            .try_recv()
            .expect("tabNavigated must reach the event sink");
        assert_eq!(event["type"], "tabNavigated");
        assert_eq!(event["from"], TARGET);

        // The follow-up navigate gets its own reply — proves no stream desync.
        front
            .navigate_to(&mut transport, "https://example.com")
            .unwrap();

        t.join().unwrap();
    }
}
