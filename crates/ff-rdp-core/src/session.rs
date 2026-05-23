//! Session — transport + actor registry bundled as a unit.
//!
//! A [`Session`] is the single object a command or daemon path needs to
//! interact with Firefox: it owns the [`RdpTransport`] and an [`Arc<Registry>`]
//! that tracks every live actor handle.
//!
//! # Transport ownership model
//!
//! `Session` owns the [`RdpTransport`] directly (not behind an `Arc<Mutex<>>`).
//! Commands borrow it mutably via [`Session::transport_mut`].  This avoids the
//! overhead and complexity of interior mutability for the single-threaded CLI
//! use-case.
//!
//! The daemon splits the transport into separate reader/writer threads *before*
//! constructing a `Session`, so it manages the transport halves independently.
//! It still creates an `Arc<Registry>` directly and shares it across threads.
//!
//! # Registry call sites
//!
//! Every command path that needs a front calls [`Session::registry`] and looks
//! up the actor by ID.  The registry is pre-populated during connection setup
//! (see [`ConnectedTab`] in `ff-rdp-cli`) so lookups are O(1) reads of the
//! `DashMap`.
//!
//! # TODO (Theme D, iter-61t)
//!
//! When `ResourceCommand` (iter-61q) is wired in, add:
//! ```rust,ignore
//! resource_command: Arc<Mutex<ResourceCommand>>
//! ```
//! and expose it via `Session::resource_command()`.  Out of scope for Theme A.

use std::sync::Arc;

use crate::registry::Registry;
use crate::transport::RdpTransport;

/// The live session for a single Firefox connection.
///
/// Cheap to pass around by `&mut` — the registry is `Arc`-wrapped and costs
/// nothing to share; the transport is borrowed, not copied.
pub struct Session {
    transport: RdpTransport,
    registry: Arc<Registry>,
    // TODO(iter-61t Theme D): add resource_command: Arc<Mutex<ResourceCommand>>
}

impl Session {
    /// Create a new `Session` wrapping the given transport.
    ///
    /// A fresh, empty [`Registry`] is created automatically.  Callers that
    /// need to pre-populate the registry (e.g. after `getTarget`) should call
    /// [`Session::registry`] and call `register` on the returned handle.
    pub fn new(transport: RdpTransport) -> Self {
        Self {
            transport,
            registry: Arc::new(Registry::new()),
        }
    }

    /// Create a `Session` from an existing transport + registry.
    ///
    /// Used by the daemon to attach a shared registry to a per-client session.
    pub fn with_registry(transport: RdpTransport, registry: Arc<Registry>) -> Self {
        Self {
            transport,
            registry,
        }
    }

    /// Borrow the transport mutably for sending/receiving RDP packets.
    pub fn transport_mut(&mut self) -> &mut RdpTransport {
        &mut self.transport
    }

    /// Return a reference to the shared actor registry.
    ///
    /// The returned `Arc` is cheap to clone if a command needs to hold on
    /// to a registry reference past the borrow of `self`.
    pub fn registry(&self) -> &Arc<Registry> {
        &self.registry
    }

    /// Clone the `Arc<Registry>` handle without cloning the registry itself.
    pub fn registry_arc(&self) -> Arc<Registry> {
        Arc::clone(&self.registry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::FrontKind;
    use crate::types::ActorId;

    fn make_session() -> Session {
        // We can't easily make a real RdpTransport in a unit test without a
        // network socket.  Use the Session::new constructor to verify the
        // registry starts empty.
        //
        // SAFETY invariant: we need a valid transport to call Session::new,
        // but for these registry-only tests we only inspect the registry.
        // Use a loopback TCP pair so the transport is valid but idle.
        use std::io::BufReader;
        use std::net::{TcpListener, TcpStream};
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let client = TcpStream::connect(addr).unwrap();
        let (server, _) = listener.accept().unwrap();
        drop(server);
        let writer = client.try_clone().unwrap();
        let reader = BufReader::new(client);
        Session::new(RdpTransport::from_parts(reader, writer))
    }

    #[test]
    fn new_session_has_empty_registry() {
        let session = make_session();
        assert!(session.registry().is_empty());
    }

    #[test]
    fn registry_registration_is_visible_through_session() {
        let session = make_session();
        let id = ActorId::from("conn0/console1");
        session
            .registry()
            .register(id.clone(), FrontKind::Console, None);
        assert!(!session.registry().is_empty());
        assert!(session.registry().assert_alive(&id).is_ok());
    }

    #[test]
    fn registry_arc_returns_same_instance() {
        let session = make_session();
        let arc1 = session.registry_arc();
        let arc2 = session.registry_arc();
        // Both Arcs must point to the same allocation.
        assert!(Arc::ptr_eq(&arc1, &arc2));
    }

    #[test]
    fn with_registry_shares_existing_registry() {
        use std::io::BufReader;
        use std::net::{TcpListener, TcpStream};

        let reg = Arc::new(Registry::new());
        let id = ActorId::from("conn0/target1");
        reg.register(id.clone(), FrontKind::Target, None);

        // Build a transport pair.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let client = TcpStream::connect(addr).unwrap();
        let (_server, _) = listener.accept().unwrap();
        let writer = client.try_clone().unwrap();
        let reader = BufReader::new(client);
        let transport = RdpTransport::from_parts(reader, writer);

        let session = Session::with_registry(transport, Arc::clone(&reg));
        // The session's registry must be the same Arc.
        assert!(Arc::ptr_eq(session.registry(), &reg));
        // The pre-registered actor is visible.
        assert!(session.registry().assert_alive(&id).is_ok());
    }
}
