//! Resource subscription bus — `ResourceCommand`.
//!
//! The `ResourceCommand` is the central in-process bus for Firefox DevTools
//! resource subscriptions.  It is modelled after the Firefox DevTools
//! `ResourceCommand.js` (see `devtools/shared/resources/ResourceCommand.js`).
//!
//! # Design (option b — fast path)
//!
//! The bus holds the watcher actor ID and calls [`WatcherActor`] static helpers
//! directly, passing in a transport reference.  This avoids moving methods onto
//! [`WatcherFront`] while reusing all existing request/response helpers.
//!
//! Key invariants:
//! - Each `ResourceType` is subscribed to on the wire at most **once**, regardless
//!   of how many in-process subscribers have requested it.  The bus maintains a
//!   reference-count per type; the last `unsubscribe` triggers `unwatchResources`.
//! - Callers receive a `Receiver<Resource>` channel end from [`ResourceCommand::subscribe`];
//!   the bus keeps the `Sender` end and fans out each event by cloning and sending to every
//!   subscriber that requested the matching type.
//! - Subscription/unsubscription manipulates the in-process state only; I/O is
//!   performed by the caller-supplied transport at `subscribe` / `unsubscribe` time.

use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};

use serde_json::Value;

use crate::actors::watcher::{
    WatcherActor, parse_console_resources, parse_network_resource_updates, parse_network_resources,
};
use crate::error::ProtocolError;
use crate::resources::resource::Resource;
use crate::resources::resource_type::ResourceType;
use crate::transport::RdpTransport;
use crate::types::ActorId;

// ---------------------------------------------------------------------------
// SubscriptionId
// ---------------------------------------------------------------------------

/// Opaque token returned by [`ResourceCommand::subscribe`].
///
/// Pass it to [`ResourceCommand::unsubscribe`] to cancel the subscription.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SubscriptionId(u64);

// ---------------------------------------------------------------------------
// Internal subscriber record
// ---------------------------------------------------------------------------

struct Subscriber {
    id: SubscriptionId,
    types: Vec<ResourceType>,
    tx: Sender<Resource>,
}

// ---------------------------------------------------------------------------
// ResourceCommand
// ---------------------------------------------------------------------------

/// Central in-process bus for Firefox resource subscriptions.
///
/// Call [`subscribe`](Self::subscribe) to request events for one or more
/// resource types.  Call [`dispatch_event`](Self::dispatch_event) from your
/// event-receive loop to fan out each incoming `resources-available-array` /
/// `resources-updated-array` packet to all matching subscribers.
///
/// # Thread safety
///
/// `ResourceCommand` is designed to be owned by a single thread; all methods
/// take `&mut self`.  The `Receiver<Resource>` ends returned by `subscribe` are
/// `Send` and may be moved to other threads.
pub struct ResourceCommand {
    /// The Firefox Watcher actor ID used for all `watchResources` calls.
    watcher_actor: ActorId,
    /// Monotonically-increasing counter for subscription IDs.
    next_sub_id: u64,
    /// All active in-process subscribers.
    subscribers: Vec<Subscriber>,
    /// Reference-count per resource type.  When > 0 the type is subscribed on
    /// the wire; when it drops to 0 we call `unwatchResources`.
    ref_counts: HashMap<ResourceType, u32>,
}

impl ResourceCommand {
    /// Create a new bus bound to `watcher_actor`.
    pub fn new(watcher_actor: ActorId) -> Self {
        Self {
            watcher_actor,
            next_sub_id: 1,
            subscribers: Vec::new(),
            ref_counts: HashMap::new(),
        }
    }

    /// Subscribe to `types`.  Returns a `(SubscriptionId, Receiver<Resource>)`.
    ///
    /// For any type not already on the wire, this sends a `watchResources`
    /// request via `transport`.  Types already subscribed by another subscriber
    /// are reused (no extra wire call).
    ///
    /// Errors only from `watchResources` (network/protocol); in-process state is
    /// not modified on error.
    pub fn subscribe(
        &mut self,
        transport: &mut RdpTransport,
        types: &[ResourceType],
    ) -> Result<(SubscriptionId, Receiver<Resource>), ProtocolError> {
        // Deduplicate the caller-supplied slice so that duplicate entries don't
        // corrupt ref-counts or produce duplicate wire strings.
        let mut deduped: Vec<ResourceType> = Vec::with_capacity(types.len());
        for &t in types {
            if !deduped.contains(&t) {
                deduped.push(t);
            }
        }

        // Find types that need a new wire subscription.
        let new_wire_types: Vec<ResourceType> = deduped
            .iter()
            .filter(|t| self.ref_counts.get(t).copied().unwrap_or(0) == 0)
            .copied()
            .collect();

        if !new_wire_types.is_empty() {
            let wire_strs: Vec<&str> = new_wire_types.iter().map(|t| t.as_wire_str()).collect();
            WatcherActor::watch_resources(transport, &self.watcher_actor, &wire_strs)?;
        }

        // Commit: increment ref-counts and register subscriber.
        for &t in &deduped {
            *self.ref_counts.entry(t).or_insert(0) += 1;
        }

        let id = SubscriptionId(self.next_sub_id);
        self.next_sub_id += 1;
        let (tx, rx) = mpsc::channel::<Resource>();
        self.subscribers.push(Subscriber {
            id,
            types: deduped,
            tx,
        });

        Ok((id, rx))
    }

    /// Unsubscribe `id`.  For any type whose ref-count drops to zero this sends
    /// `unwatchResources` via `transport`.
    ///
    /// Returns `Ok(())` if `id` was not found (idempotent).
    /// Wire errors from `unwatchResources` are returned but in-process state is
    /// already updated (the subscriber is gone).
    pub fn unsubscribe(
        &mut self,
        transport: &mut RdpTransport,
        id: SubscriptionId,
    ) -> Result<(), ProtocolError> {
        let pos = self.subscribers.iter().position(|s| s.id == id);
        let Some(pos) = pos else {
            return Ok(());
        };

        let sub = self.subscribers.remove(pos);

        // Decrement ref-counts and collect types that hit zero.
        let mut to_unwatch: Vec<ResourceType> = Vec::new();
        for t in &sub.types {
            let count = self.ref_counts.entry(*t).or_insert(0);
            *count = count.saturating_sub(1);
            if *count == 0 {
                to_unwatch.push(*t);
            }
        }

        if !to_unwatch.is_empty() {
            let wire_strs: Vec<&str> = to_unwatch.iter().map(|t| t.as_wire_str()).collect();
            WatcherActor::unwatch_resources(transport, &self.watcher_actor, &wire_strs)?;
        }

        Ok(())
    }

    /// Dispatch a raw Firefox RDP event packet to all matching subscribers.
    ///
    /// Call this for every `resources-available-array` and
    /// `resources-updated-array` message received from the transport.
    /// The bus parses the packet once and fans out typed [`Resource`] events
    /// to every subscriber that requested the matching type.
    ///
    /// Packets for unknown resource types are silently ignored.
    /// Dead subscriber channels (dropped receivers) are cleaned up lazily on
    /// each call.
    pub fn dispatch_event(&mut self, event: &Value) {
        let msg_type = event
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();

        let resources: Vec<Resource> = match msg_type {
            "resources-available-array" => Self::parse_available_resources(event),
            "resources-updated-array" => parse_network_resource_updates(event)
                .into_iter()
                .map(Resource::NetworkUpdate)
                .collect(),
            _ => return,
        };

        // Fan out and collect dead subscriber IDs.
        let mut dead: Vec<SubscriptionId> = Vec::new();
        for resource in resources {
            let type_name = resource.type_name();
            let rt = ResourceType::from_wire_str(type_name);
            for sub in &self.subscribers {
                let wants = rt.is_some_and(|t| sub.types.contains(&t));
                if wants && sub.tx.send(resource.clone()).is_err() {
                    dead.push(sub.id);
                }
            }
        }

        // Prune dead channels.  We must also decrement ref-counts for the
        // pruned subscribers' types so that wire subscriptions don't leak and
        // subsequent `unsubscribe` calls can still send `unwatchResources`.
        if !dead.is_empty() {
            dead.sort_unstable_by_key(|id| id.0);
            dead.dedup();
            // Partition into dead and live; collect types from dead subscribers.
            let mut i = 0;
            while i < self.subscribers.len() {
                if dead.contains(&self.subscribers[i].id) {
                    let removed = self.subscribers.swap_remove(i);
                    for t in &removed.types {
                        let count = self.ref_counts.entry(*t).or_insert(0);
                        *count = count.saturating_sub(1);
                    }
                    // Don't advance i — the swap put a new element at position i.
                } else {
                    i += 1;
                }
            }
        }
    }

    /// Parse a `resources-available-array` event into typed [`Resource`] items.
    fn parse_available_resources(event: &Value) -> Vec<Resource> {
        let mut out = Vec::new();

        let Some(array) = event.get("array").and_then(Value::as_array) else {
            return out;
        };

        for sub in array {
            let sub_arr = match sub.as_array() {
                Some(a) if a.len() == 2 => a,
                _ => continue,
            };

            let resource_type_str = sub_arr[0].as_str().unwrap_or_default();

            match resource_type_str {
                "network-event" => {
                    // Re-wrap for the existing parser.
                    let wrapped =
                        serde_json::json!({"array": [["network-event", sub_arr[1].clone()]]});
                    for r in parse_network_resources(&wrapped) {
                        out.push(Resource::NetworkEvent(r));
                    }
                }
                "console-message" => {
                    let wrapped =
                        serde_json::json!({"array": [["console-message", sub_arr[1].clone()]]});
                    for r in parse_console_resources(&wrapped) {
                        out.push(Resource::ConsoleMessage(r));
                    }
                }
                "error-message" => {
                    let wrapped =
                        serde_json::json!({"array": [["error-message", sub_arr[1].clone()]]});
                    for r in parse_console_resources(&wrapped) {
                        out.push(Resource::ErrorMessage(r));
                    }
                }
                "document-event" => {
                    if let Some(items) = sub_arr[1].as_array() {
                        for item in items {
                            out.push(Resource::DocumentEvent(item.clone()));
                        }
                    }
                }
                _ => {}
            }
        }

        out
    }

    /// Return the wire-level ref-count for `rt` (number of active subscriptions).
    ///
    /// Primarily for testing and diagnostics.
    pub fn ref_count(&self, rt: ResourceType) -> u32 {
        self.ref_counts.get(&rt).copied().unwrap_or(0)
    }

    /// Return the number of active in-process subscribers.
    pub fn subscriber_count(&self) -> usize {
        self.subscribers.len()
    }

    /// Return the watcher actor ID bound to this bus.
    pub fn watcher_actor(&self) -> &ActorId {
        &self.watcher_actor
    }
}
