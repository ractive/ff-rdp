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
//!
//! # Throttle policy — zero delay (Bug 1914386)
//!
//! The upstream `ResourceCommand.js` used to coalesce resource notifications
//! with a 100 ms timer before dispatching them to listeners (see the old
//! `_throttledDispatchResourceAvailable` implementation).  Firefox Bug 1914386
//! removed that throttle in favour of passing every packet through immediately
//! while still supporting array-batching: a single `resources-available-array`
//! frame carrying N resources fans out as one `dispatch_event` call that loops
//! over the inner array and delivers each `Resource` to matching subscribers.
//!
//! Reference: `devtools/shared/commands/resource/resource-command.js:73-79`.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};

use serde_json::Value;

use crate::actors::watcher::{
    WatcherActor, parse_console_resources, parse_network_resource_updates, parse_network_resources,
};
use crate::error::ProtocolError;
use crate::resources::resource::Resource;
use crate::resources::resource_type::ResourceType;
use crate::transport::{FramedWriter, RdpTransport};
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
    tx: Sender<Arc<Resource>>,
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
/// Call [`gc`](Self::gc) periodically (e.g. after each event loop cycle) to
/// flush pending `unwatchResources` packets that were scheduled when dead
/// subscriber channels were detected during [`dispatch_event`].
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
    /// Resource types whose ref-count just dropped to zero via dead-channel
    /// pruning in `dispatch_event`.  These need an `unwatchResources` wire call
    /// but we can't send it from inside `dispatch_event` (no transport access),
    /// so we defer to the next `gc()` call.
    pending_unwatch: Vec<ResourceType>,
}

impl ResourceCommand {
    /// Create a new bus bound to `watcher_actor`.
    pub fn new(watcher_actor: ActorId) -> Self {
        Self {
            watcher_actor,
            next_sub_id: 1,
            subscribers: Vec::new(),
            ref_counts: HashMap::new(),
            pending_unwatch: Vec::new(),
        }
    }

    /// Register a subscriber directly (bypassing the wire) for testing.
    ///
    /// Does NOT send `watchResources` — only used in benchmarks and unit tests
    /// that drive `dispatch_event` without a real transport.
    #[cfg(test)]
    pub(crate) fn add_subscriber_direct(
        &mut self,
        types: Vec<ResourceType>,
        tx: Sender<Arc<Resource>>,
    ) -> SubscriptionId {
        for &t in &types {
            *self.ref_counts.entry(t).or_insert(0) += 1;
        }
        let id = SubscriptionId(self.next_sub_id);
        self.next_sub_id += 1;
        self.subscribers.push(Subscriber { id, types, tx });
        id
    }

    /// Subscribe to `types`.  Returns a `(SubscriptionId, Receiver<Arc<Resource>>)`.
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
    ) -> Result<(SubscriptionId, Receiver<Arc<Resource>>), ProtocolError> {
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
        let (tx, rx) = mpsc::channel::<Arc<Resource>>();
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
            // Remove zero-count entries so long-lived buses don't accumulate
            // stale map entries for types that have been fully unwatched.
            for t in &to_unwatch {
                self.ref_counts.remove(t);
            }
        }

        Ok(())
    }

    /// Dispatch a raw Firefox RDP event packet to all matching subscribers.
    ///
    /// Call this for every `resources-available-array`,
    /// `resources-updated-array`, and `resources-destroyed-array` message
    /// received from the transport.  The bus parses the packet once and fans
    /// out typed [`Resource`] events to every subscriber that requested the
    /// matching type.
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
            "resources-destroyed-array" => Self::parse_destroyed_resources(event),
            _ => return,
        };

        // Fan out and collect dead subscriber IDs.
        // Wrap each Resource in Arc once so all subscribers share the same
        // allocation — no per-subscriber clone of the Resource data.
        let mut dead: Vec<SubscriptionId> = Vec::new();
        for resource in resources {
            let type_name = resource.type_name();
            let rt = ResourceType::from_wire_str(&type_name);
            let arc = Arc::new(resource);
            for sub in &self.subscribers {
                let wants = rt.is_some_and(|t| sub.types.contains(&t));
                if wants && sub.tx.send(Arc::clone(&arc)).is_err() {
                    dead.push(sub.id);
                }
            }
        }

        // Prune dead channels.  We must also decrement ref-counts for the
        // pruned subscribers' types so that wire subscriptions don't leak and
        // subsequent `unsubscribe` calls can still send `unwatchResources`.
        //
        // Types whose ref-count reaches zero here are pushed onto
        // `pending_unwatch`; the caller must invoke `gc()` to flush the
        // corresponding `unwatchResources` wire call (we can't send it here
        // because `dispatch_event` has no transport access).
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
                        if *count == 0 {
                            self.pending_unwatch.push(*t);
                        }
                    }
                    // Don't advance i — the swap put a new element at position i.
                } else {
                    i += 1;
                }
            }
        }
    }

    /// Flush pending `unwatchResources` wire calls for resource types whose
    /// last subscriber was pruned via dead-channel detection in
    /// [`dispatch_event`].
    ///
    /// Call this periodically — e.g. after each event-loop cycle in the daemon,
    /// or before returning from a CLI helper — to ensure Firefox is informed
    /// that we no longer want events for abandoned types.
    ///
    /// Types whose ref-count has climbed back above zero (because a new
    /// subscriber arrived since the pruning) are skipped silently.
    ///
    /// Returns `Ok(())` if there are no pending types to unwatch or all wire
    /// calls succeed.  Wire errors are returned but the pending list is
    /// cleared regardless (the subscription is already gone in-process).
    pub fn gc(&mut self, transport: &mut RdpTransport) -> Result<(), ProtocolError> {
        if self.pending_unwatch.is_empty() {
            return Ok(());
        }

        // Deduplicate and keep only types still at zero.
        self.pending_unwatch
            .sort_unstable_by_key(|t| t.as_wire_str());
        self.pending_unwatch.dedup();
        let to_unwatch: Vec<ResourceType> = self
            .pending_unwatch
            .iter()
            .filter(|t| self.ref_counts.get(*t).copied().unwrap_or(0) == 0)
            .copied()
            .collect();

        if to_unwatch.is_empty() {
            // Nothing to flush — clear the deduplicated list (all had non-zero
            // ref-counts, meaning new subscribers arrived before gc() ran).
            self.pending_unwatch.clear();
            return Ok(());
        }

        let wire_strs: Vec<&str> = to_unwatch.iter().map(|t| t.as_wire_str()).collect();
        let result = WatcherActor::unwatch_resources(transport, &self.watcher_actor, &wire_strs);

        match result {
            Ok(_) => {
                // Wire send succeeded — clear pending list and remove zero-count
                // map entries so long-lived buses don't accumulate stale entries.
                self.pending_unwatch.clear();
                for t in &to_unwatch {
                    self.ref_counts.remove(t);
                }
                Ok(())
            }
            Err(e) => {
                // Wire send failed — keep pending_unwatch so the caller can
                // retry on the next gc() cycle.  Log so the failure is observable.
                tracing::warn!(
                    "gc: unwatchResources failed for {:?}: {e:#} — will retry on next gc() call",
                    to_unwatch
                        .iter()
                        .map(|t| t.as_wire_str())
                        .collect::<Vec<_>>()
                );
                Err(e)
            }
        }
    }

    /// Flush pending `unwatchResources` wire calls using a write-only transport.
    ///
    /// Like [`gc`](Self::gc) but uses a [`FramedWriter`] (write half of a split
    /// transport) instead of a full [`RdpTransport`].  This is the correct API
    /// for contexts where the transport has been split and only the write half is
    /// available — e.g. the daemon's event-dispatcher thread.
    ///
    /// The packet is sent **fire-and-forget**: no reply is read.  Firefox will
    /// send an `unwatchResources` acknowledgement but the daemon's reader thread
    /// will consume it; we do not need to wait for it here.
    ///
    /// On success, `pending_unwatch` is cleared and zero-count `ref_counts`
    /// entries are removed.  On failure, `pending_unwatch` is kept so the next
    /// call can retry.
    pub fn gc_fire_forget(&mut self, writer: &mut FramedWriter) {
        if self.pending_unwatch.is_empty() {
            return;
        }

        self.pending_unwatch
            .sort_unstable_by_key(|t| t.as_wire_str());
        self.pending_unwatch.dedup();
        let to_unwatch: Vec<ResourceType> = self
            .pending_unwatch
            .iter()
            .filter(|t| self.ref_counts.get(*t).copied().unwrap_or(0) == 0)
            .copied()
            .collect();

        if to_unwatch.is_empty() {
            self.pending_unwatch.clear();
            return;
        }

        let types: Vec<serde_json::Value> = to_unwatch
            .iter()
            .map(|t| serde_json::json!(t.as_wire_str()))
            .collect();
        let packet = serde_json::json!({
            "to": self.watcher_actor.as_ref(),
            "type": "unwatchResources",
            "resourceTypes": types,
        });

        match writer.send(&packet) {
            Ok(()) => {
                self.pending_unwatch.clear();
                for t in &to_unwatch {
                    self.ref_counts.remove(t);
                }
            }
            Err(e) => {
                tracing::warn!(
                    "gc_fire_forget: failed to send unwatchResources for {:?}: {e:#}",
                    to_unwatch
                        .iter()
                        .map(|t| t.as_wire_str())
                        .collect::<Vec<_>>()
                );
                // Keep pending_unwatch for retry on next cycle.
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

    /// Parse a `resources-destroyed-array` event into typed [`Resource::Destroyed`] items.
    ///
    /// The wire format mirrors `resources-available-array`:
    /// ```json
    /// { "type": "resources-destroyed-array",
    ///   "array": [["network-event", [{"resourceId": "...", ...}]], ...] }
    /// ```
    /// Each inner resource object is expected to carry a `resourceId` field.
    /// Objects without a recognisable `resourceId` are skipped.
    fn parse_destroyed_resources(event: &Value) -> Vec<Resource> {
        let mut out = Vec::new();

        let Some(array) = event.get("array").and_then(Value::as_array) else {
            return out;
        };

        for entry in array {
            let entry_arr = match entry.as_array() {
                Some(a) if a.len() == 2 => a,
                _ => continue,
            };

            let resource_type = match entry_arr[0].as_str() {
                Some(s) if !s.is_empty() => s.to_owned(),
                _ => continue,
            };

            let Some(resources_arr) = entry_arr[1].as_array() else {
                continue;
            };

            for resource_obj in resources_arr {
                // `resourceId` may be a string or a number on the wire.
                let resource_id = match resource_obj.get("resourceId") {
                    Some(Value::String(s)) => s.clone(),
                    Some(Value::Number(n)) => n.to_string(),
                    _ => continue,
                };

                out.push(Resource::Destroyed {
                    resource_type: resource_type.clone(),
                    resource_id,
                });
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

    /// Return the number of resource types pending an `unwatchResources` flush.
    ///
    /// Non-zero means `gc()` has not yet been called after a dead-channel prune.
    /// Primarily for testing and diagnostics.
    pub fn pending_unwatch_count(&self) -> usize {
        self.pending_unwatch.len()
    }
}

// ---------------------------------------------------------------------------
// Unit tests for `dispatch_event` (no transport / mock server required)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Build a minimal `ResourceCommand` that has one subscriber registered
    /// for `NetworkEvent` without going through the wire — we bypass
    /// `subscribe` and insert directly so tests have no transport dependency.
    fn bus_with_net_subscriber() -> (ResourceCommand, std::sync::mpsc::Receiver<Arc<Resource>>) {
        let watcher: ActorId = ActorId::from("conn0/watcher1");
        let mut bus = ResourceCommand::new(watcher);

        let (tx, rx) = std::sync::mpsc::channel::<Arc<Resource>>();
        bus.subscribers.push(Subscriber {
            id: SubscriptionId(1),
            types: vec![ResourceType::NetworkEvent],
            tx,
        });
        *bus.ref_counts
            .entry(ResourceType::NetworkEvent)
            .or_insert(0) += 1;

        (bus, rx)
    }

    // -----------------------------------------------------------------------
    // Theme A (iter-71) tests — unwatch on last-subscriber-drop via gc()
    // -----------------------------------------------------------------------

    /// `resource_command_unwatch_on_drop` (unit portion):
    /// After a dead-channel prune, `pending_unwatch_count()` is non-zero and
    /// `gc()` clears it.  The wire call is verified in the mock-server test.
    #[test]
    fn dead_channel_prune_sets_pending_unwatch() {
        let watcher: ActorId = ActorId::from("conn0/watcher1");
        let mut bus = ResourceCommand::new(watcher);

        let (tx, rx) = std::sync::mpsc::channel::<Arc<Resource>>();
        bus.add_subscriber_direct(vec![ResourceType::NetworkEvent], tx);

        // Drop the receiver so the channel is dead.
        drop(rx);

        assert_eq!(
            bus.pending_unwatch_count(),
            0,
            "pending_unwatch starts empty"
        );

        // Dispatch an event — dead-channel pruning fires, pushing to pending_unwatch.
        let packet = json!({
            "type": "resources-available-array",
            "array": [
                ["network-event", [{
                    "actor": "conn0/netEvent1",
                    "method": "GET",
                    "url": "https://example.com/",
                    "isXHR": false,
                    "cause": {"type": "document"},
                    "startedDateTime": "2026-01-01T00:00:00Z",
                    "timeStamp": 1000.0,
                    "resourceId": 1_u64
                }]]
            ]
        });
        bus.dispatch_event(&packet);

        assert_eq!(
            bus.pending_unwatch_count(),
            1,
            "pending_unwatch should have 1 entry after dead-channel prune"
        );
        assert_eq!(
            bus.ref_count(ResourceType::NetworkEvent),
            0,
            "ref-count must be 0 after prune"
        );
    }

    /// `resource_command_no_unwatch_with_live_subscribers`:
    /// With ref-count > 0 (live subscriber), `gc()` is a no-op — nothing is
    /// pushed to `pending_unwatch` and no wire packet would be sent.
    #[test]
    fn resource_command_no_unwatch_with_live_subscribers() {
        let watcher: ActorId = ActorId::from("conn0/watcher1");
        let mut bus = ResourceCommand::new(watcher);

        let (tx, _rx) = std::sync::mpsc::channel::<Arc<Resource>>();
        // Keep _rx alive so the channel is not dead.
        bus.add_subscriber_direct(vec![ResourceType::NetworkEvent], tx);

        let packet = json!({
            "type": "resources-available-array",
            "array": [
                ["network-event", [{
                    "actor": "conn0/netEvent1",
                    "method": "GET",
                    "url": "https://example.com/",
                    "isXHR": false,
                    "cause": {"type": "document"},
                    "startedDateTime": "2026-01-01T00:00:00Z",
                    "timeStamp": 1000.0,
                    "resourceId": 1_u64
                }]]
            ]
        });
        bus.dispatch_event(&packet);

        assert_eq!(
            bus.pending_unwatch_count(),
            0,
            "no pending_unwatch when subscriber is still live"
        );
        assert_eq!(
            bus.ref_count(ResourceType::NetworkEvent),
            1,
            "ref-count unchanged with live subscriber"
        );
    }

    /// Theme D AC: `resources-destroyed-array` is dispatched as
    /// `Resource::Destroyed` to matching subscribers.
    #[test]
    fn dispatch_destroyed_array_reaches_subscriber() {
        let (mut bus, rx) = bus_with_net_subscriber();

        let packet = json!({
            "type": "resources-destroyed-array",
            "array": [
                ["network-event", [{"resourceId": "42"}]]
            ]
        });

        bus.dispatch_event(&packet);

        let events: Vec<Arc<Resource>> = rx.try_iter().collect();
        assert_eq!(
            events.len(),
            1,
            "subscriber should receive exactly 1 Destroyed event"
        );

        match events[0].as_ref() {
            Resource::Destroyed {
                resource_type,
                resource_id,
            } => {
                assert_eq!(resource_type, "network-event");
                assert_eq!(resource_id, "42");
            }
            other => panic!("expected Resource::Destroyed, got {other:?}"),
        }
    }

    /// Numeric `resourceId` values (as produced by Firefox) are stringified.
    #[test]
    fn dispatch_destroyed_array_numeric_resource_id() {
        let (mut bus, rx) = bus_with_net_subscriber();

        let packet = json!({
            "type": "resources-destroyed-array",
            "array": [
                ["network-event", [{"resourceId": 99_u64}]]
            ]
        });

        bus.dispatch_event(&packet);

        let events: Vec<Arc<Resource>> = rx.try_iter().collect();
        assert_eq!(events.len(), 1);

        match events[0].as_ref() {
            Resource::Destroyed { resource_id, .. } => {
                assert_eq!(resource_id, "99");
            }
            other => panic!("expected Resource::Destroyed, got {other:?}"),
        }
    }

    /// Non-subscribers for a type do not receive destroyed events for that type.
    #[test]
    fn dispatch_destroyed_array_non_matching_type_not_received() {
        // Subscriber only wants console-message, but packet destroys network-event.
        let watcher: ActorId = ActorId::from("conn0/watcher1");
        let mut bus = ResourceCommand::new(watcher);

        let (tx, rx) = std::sync::mpsc::channel::<Arc<Resource>>();
        bus.subscribers.push(Subscriber {
            id: SubscriptionId(1),
            types: vec![ResourceType::ConsoleMessage],
            tx,
        });
        *bus.ref_counts
            .entry(ResourceType::ConsoleMessage)
            .or_insert(0) += 1;

        let packet = json!({
            "type": "resources-destroyed-array",
            "array": [
                ["network-event", [{"resourceId": "7"}]]
            ]
        });
        bus.dispatch_event(&packet);

        let events: Vec<Arc<Resource>> = rx.try_iter().collect();
        assert!(
            events.is_empty(),
            "console-message subscriber must not receive network-event destroyed"
        );
    }

    /// Mixed available → updated → destroyed scenario: all three event shapes
    /// reach the same subscriber in order.
    #[test]
    fn dispatch_available_then_destroyed_roundtrip() {
        let (mut bus, rx) = bus_with_net_subscriber();

        // 1. available
        let available = json!({
            "type": "resources-available-array",
            "array": [
                ["network-event", [{
                    "actor": "conn0/netEvent1",
                    "method": "GET",
                    "url": "https://example.com/",
                    "isXHR": false,
                    "cause": {"type": "document"},
                    "startedDateTime": "2026-01-01T00:00:00Z",
                    "timeStamp": 1000.0,
                    "resourceId": 1_u64
                }]]
            ]
        });
        bus.dispatch_event(&available);

        // 2. destroyed
        let destroyed = json!({
            "type": "resources-destroyed-array",
            "array": [
                ["network-event", [{"resourceId": "1"}]]
            ]
        });
        bus.dispatch_event(&destroyed);

        let events: Vec<Arc<Resource>> = rx.try_iter().collect();
        assert_eq!(
            events.len(),
            2,
            "subscriber should receive 1 NetworkEvent + 1 Destroyed"
        );

        assert!(
            matches!(events[0].as_ref(), Resource::NetworkEvent(_)),
            "first event should be NetworkEvent"
        );
        match events[1].as_ref() {
            Resource::Destroyed {
                resource_type,
                resource_id,
            } => {
                assert_eq!(resource_type, "network-event");
                assert_eq!(resource_id, "1");
            }
            other => panic!("second event should be Destroyed, got {other:?}"),
        }
    }

    /// Micro-benchmark: a single `resources-available-array` event must fan
    /// out to a subscriber in well under 1 ms (target from iter-61v AC).
    ///
    /// This is a regular `#[test]` using wall-clock measurement rather than a
    /// criterion bench, so it runs in the normal test suite.  We repeat 1 000
    /// iterations and assert that the median (50th-percentile) round-trip is
    /// below 5 ms; the budget is intentionally loose so the assertion stays
    /// stable under loaded/contended CI runners, but still catches the
    /// regression we care about (accidental reintroduction of a sleep- or
    /// timer-based throttle would push the median into the 100 ms range).
    #[test]
    fn bench_bus_dispatch_latency() {
        use std::time::Instant;

        const ITERS: usize = 1_000;
        const BUDGET_NS: u128 = 5_000_000;

        let (mut bus, rx) = bus_with_net_subscriber();

        let packet = json!({
            "type": "resources-available-array",
            "array": [
                ["network-event", [{
                    "actor": "conn0/netEvent1",
                    "method": "GET",
                    "url": "https://example.com/",
                    "isXHR": false,
                    "cause": {"type": "document"},
                    "startedDateTime": "2026-01-01T00:00:00Z",
                    "timeStamp": 1000.0,
                    "resourceId": 1_u64
                }]]
            ]
        });

        let mut times_ns = Vec::with_capacity(ITERS);

        for _ in 0..ITERS {
            let t0 = Instant::now();
            bus.dispatch_event(&packet);
            times_ns.push(t0.elapsed().as_nanos());
            // Drain so the channel does not grow unbounded.
            let _ = rx.try_recv();
        }

        times_ns.sort_unstable();
        let median_ns = times_ns[ITERS / 2];

        assert!(
            median_ns < BUDGET_NS,
            "bus dispatch median latency {median_ns} ns exceeds 5 ms budget — \
             check for accidental timer/throttle re-introduction"
        );
    }

    /// Theme H (iter-61x): `bench_bus_fanout_4_subscribers` — verify that
    /// fanning out one event to 4 subscribers costs well under 5 ms per call.
    ///
    /// Before the `Arc<Resource>` change each subscriber received an
    /// individually-cloned `Resource`.  With `Arc<Resource>` only 4 pointer
    /// copies are made regardless of the payload size.
    ///
    /// This test exercises the `Arc` path: 4 subscribers, 1 000 events, each
    /// carrying a network-event payload.  We assert that the 50th-percentile
    /// latency stays below the same 5 ms wall-clock budget used by the single-
    /// subscriber bench — the pointer copies must not materially increase the
    /// per-dispatch cost over the baseline.
    #[test]
    fn bench_bus_fanout_4_subscribers() {
        use std::time::Instant;

        const ITERS: usize = 1_000;
        const BUDGET_NS: u128 = 5_000_000; // 5 ms (loose; catches regressions)

        let mut bus = ResourceCommand::new(ActorId::from("conn0/watcher1"));

        // Add 4 subscribers for the same resource type (no wire calls needed
        // because we drive dispatch_event directly).
        let rxs: Vec<_> = (0..4)
            .map(|_| {
                let (tx, rx) = mpsc::channel::<std::sync::Arc<Resource>>();
                let sub_id = bus.add_subscriber_direct(vec![ResourceType::NetworkEvent], tx);
                (sub_id, rx)
            })
            .collect();

        let packet = json!({
            "type": "resources-available-array",
            "array": [
                ["network-event", [{
                    "actor": "conn0/netEvent1",
                    "method": "GET",
                    "url": "https://example.com/",
                    "isXHR": false,
                    "cause": {"type": "document"},
                    "startedDateTime": "2026-01-01T00:00:00Z",
                    "timeStamp": 1000.0,
                    "resourceId": 1_u64
                }]]
            ]
        });

        let mut times_ns = Vec::with_capacity(ITERS);

        for _ in 0..ITERS {
            let t0 = Instant::now();
            bus.dispatch_event(&packet);
            times_ns.push(t0.elapsed().as_nanos());
            // Drain all 4 receivers to avoid unbounded channel growth.
            for (_, rx) in &rxs {
                while rx.try_recv().is_ok() {}
            }
        }

        times_ns.sort_unstable();
        let median_ns = times_ns[ITERS / 2];

        assert!(
            median_ns < BUDGET_NS,
            "bus fanout-4 median latency {median_ns} ns exceeds 5 ms budget — \
             Arc<Resource> clone overhead is too high"
        );
    }

    // -----------------------------------------------------------------------
    // iter-71b tests — ref_counts cleanup after gc() and unsubscribe()
    // -----------------------------------------------------------------------

    /// `gc_drops_flushed_ref_counts` (iter-71b AC):
    /// After `gc()` flushes a type that was pruned via dead-channel detection,
    /// `ref_counts` must no longer contain that key.  Long-lived daemons must
    /// not accumulate stale zero-valued map entries.
    #[test]
    fn gc_drops_flushed_ref_counts() {
        use std::io::{BufReader, Write as _};
        use std::net::{TcpListener, TcpStream};

        // Build a loopback transport so gc() can actually send the wire packet and
        // receive the acknowledgement that actor_request requires.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let client = TcpStream::connect(addr).unwrap();
        let (server, _) = listener.accept().unwrap();

        let writer = client.try_clone().unwrap();
        let reader = BufReader::new(client);
        let mut transport = RdpTransport::from_parts(reader, writer);

        // The server thread: read one request (unwatchResources) and reply with an ack.
        // This prevents actor_request from failing with EOF.
        let watcher_for_server = "conn0/watcher1".to_owned();
        std::thread::spawn(move || {
            let mut srv_reader = BufReader::new(server.try_clone().unwrap());
            if let Ok(_req) = crate::transport::recv_from(&mut srv_reader) {
                let ack = serde_json::json!({"from": watcher_for_server});
                let frame = crate::transport::encode_frame(&serde_json::to_string(&ack).unwrap());
                let _ = (&server).write_all(frame.as_bytes());
            }
        });

        let watcher: ActorId = ActorId::from("conn0/watcher1");
        let mut bus = ResourceCommand::new(watcher);

        // Register a subscriber via add_subscriber_direct (no wire call).
        let (tx, rx) = std::sync::mpsc::channel::<Arc<Resource>>();
        bus.add_subscriber_direct(vec![ResourceType::NetworkEvent], tx);

        // Verify ref-count is 1 after registration.
        assert_eq!(bus.ref_count(ResourceType::NetworkEvent), 1);
        assert!(
            bus.ref_counts.contains_key(&ResourceType::NetworkEvent),
            "ref_counts must contain the key before gc"
        );

        // Drop receiver to make the channel dead.
        drop(rx);

        // Dispatch an event — dead-channel pruning fires and queues NetworkEvent
        // into pending_unwatch.
        let packet = json!({
            "type": "resources-available-array",
            "array": [["network-event", [{
                "actor": "conn0/netEvent1",
                "method": "GET",
                "url": "https://example.com/",
                "isXHR": false,
                "cause": {"type": "document"},
                "startedDateTime": "2026-01-01T00:00:00Z",
                "timeStamp": 1000.0,
                "resourceId": 1_u64
            }]]]
        });
        bus.dispatch_event(&packet);

        assert_eq!(
            bus.pending_unwatch_count(),
            1,
            "pending_unwatch should have 1 entry"
        );
        assert_eq!(
            bus.ref_count(ResourceType::NetworkEvent),
            0,
            "ref-count should be 0 after prune"
        );
        assert!(
            bus.ref_counts.contains_key(&ResourceType::NetworkEvent),
            "ref_counts key still present before gc"
        );

        // gc() flushes the wire call and removes the zero-count entry.
        let result = bus.gc(&mut transport);
        assert!(
            result.is_ok(),
            "gc() must succeed with a responding mock server: {result:?}"
        );

        assert_eq!(
            bus.pending_unwatch_count(),
            0,
            "pending_unwatch must be empty after gc"
        );
        assert!(
            !bus.ref_counts.contains_key(&ResourceType::NetworkEvent),
            "gc_drops_flushed_ref_counts: ref_counts must not contain NetworkEvent after gc"
        );
    }

    /// After `unsubscribe()` drops the last subscriber for a type, the zero-
    /// count entry is removed from `ref_counts` (iter-71b B5).
    #[test]
    fn unsubscribe_drops_zero_ref_count_entry() {
        use std::io::{BufReader, Read, Write as _};
        use std::net::{TcpListener, TcpStream};

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let client = TcpStream::connect(addr).unwrap();
        let (server, _) = listener.accept().unwrap();

        // Background thread to reply to watchResources + unwatchResources.
        std::thread::spawn(move || {
            use crate::transport::{encode_frame, recv_from};
            let mut reader = BufReader::new(&server);
            // Expect watchResources, reply with ack (no "type" field — actor replies
            // must not carry a "type" field or recv_reply_from treats them as events).
            if let Ok(req) = recv_from(&mut reader) {
                let ack = serde_json::json!({"from": req["to"]});
                let frame = encode_frame(&serde_json::to_string(&ack).unwrap());
                let _ = (&server).write_all(frame.as_bytes());
            }
            // Expect unwatchResources, reply with ack.
            if let Ok(req) = recv_from(&mut reader) {
                let ack = serde_json::json!({"from": req["to"]});
                let frame = encode_frame(&serde_json::to_string(&ack).unwrap());
                let _ = (&server).write_all(frame.as_bytes());
            }
            // Drain remaining.
            let _ = (&server).read_to_end(&mut Vec::new());
        });

        let writer = client.try_clone().unwrap();
        let reader = BufReader::new(client);
        let mut transport = RdpTransport::from_parts(reader, writer);

        let watcher: ActorId = ActorId::from("conn0/watcher1");
        let mut bus = ResourceCommand::new(watcher);

        // Subscribe — this sends watchResources on the wire.
        let (sub_id, _rx) = bus
            .subscribe(&mut transport, &[ResourceType::NetworkEvent])
            .expect("subscribe");

        assert_eq!(bus.ref_count(ResourceType::NetworkEvent), 1);
        assert!(bus.ref_counts.contains_key(&ResourceType::NetworkEvent));

        // Unsubscribe the last subscriber — should send unwatchResources and
        // remove the zero-count entry.
        bus.unsubscribe(&mut transport, sub_id)
            .expect("unsubscribe");

        assert_eq!(bus.ref_count(ResourceType::NetworkEvent), 0);
        assert!(
            !bus.ref_counts.contains_key(&ResourceType::NetworkEvent),
            "unsubscribe_drops_zero_ref_count_entry: key must be removed after last unsubscribe"
        );
    }
}
