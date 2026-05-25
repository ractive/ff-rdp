use std::collections::BTreeMap;
use std::sync::mpsc;

use serde_json::{Value, json};

use crate::actor::actor_request;
use crate::error::ProtocolError;
use crate::transport::RdpTransport;
use crate::types::{ActorId, Grip};

// ---------------------------------------------------------------------------
// GripKind marker trait and concrete markers
// ---------------------------------------------------------------------------

/// Marker trait for the kind of server-side actor a [`ScopedGrip`] wraps.
///
/// The marker determines which release method to send to Firefox on drop:
/// - [`ObjectGrip`] → `"release"` (per `devtools/shared/specs/object.js:213`).
/// - [`LongStringGrip`] → `"release"` (per `devtools/server/actors/string.js:40-50`).
///
/// Both currently use `"release"` as the method name; the separate marker
/// types exist to enforce type-safety at call sites (you cannot accidentally
/// release a long-string via an object actor).
pub trait GripKind: sealed::Sealed {
    /// The Firefox RDP method name to invoke when releasing this kind.
    const RELEASE_METHOD: &'static str;
}

mod sealed {
    pub trait Sealed {}
}

/// Marker: the wrapped actor is a JavaScript object grip.
pub struct ObjectGrip;

impl sealed::Sealed for ObjectGrip {}
impl GripKind for ObjectGrip {
    const RELEASE_METHOD: &'static str = "release";
}

/// Marker: the wrapped actor is a long-string grip.
pub struct LongStringGrip;

impl sealed::Sealed for LongStringGrip {}
impl GripKind for LongStringGrip {
    const RELEASE_METHOD: &'static str = "release";
}

// ---------------------------------------------------------------------------
// Release queue
// ---------------------------------------------------------------------------

/// A pending release request enqueued by a dropped [`GripHandle<K>`].
///
/// The actor ID and release method are recorded at drop time so the queue
/// can be drained later — either by the demux reader thread (daemon mode)
/// or by the next `actor_request` call (synchronous CLI mode).
#[derive(Debug)]
pub struct ReleaseRequest {
    /// The actor ID to send the release packet to.
    pub actor_id: ActorId,
    /// The Firefox RDP method name to invoke (e.g. `"release"`).
    pub method: &'static str,
}

/// Sender half of the release queue, shared across all [`ScopedGrip`] instances.
///
/// Obtained by calling [`release_queue`].  Pass the matching receiver to a
/// background drainer (daemon mode) or drain inline (synchronous mode).
pub type ReleaseQueueTx = mpsc::SyncSender<ReleaseRequest>;

/// Receiver half of the release queue.
pub type ReleaseQueueRx = mpsc::Receiver<ReleaseRequest>;

/// Create a bounded release queue with the given `capacity`.
///
/// Typically called once per connection at setup time.  The returned `Tx` is
/// cloned into every [`ScopedGrip`] created for that connection.  The `Rx` is
/// handed to a drainer (background thread or next-request inline drain).
///
/// If the queue is full when a `ScopedGrip` is dropped, the release is silently
/// discarded — the actor leaks until the connection closes, which is acceptable
/// as a fallback under extreme load.
pub fn release_queue(capacity: usize) -> (ReleaseQueueTx, ReleaseQueueRx) {
    mpsc::sync_channel(capacity)
}

/// A property descriptor as returned by Firefox's `prototypeAndProperties`.
#[derive(Debug, Clone)]
pub enum PropertyDescriptor {
    /// Data property with a value.
    Data {
        value: Grip,
        writable: bool,
        enumerable: bool,
        configurable: bool,
    },
    /// Accessor property with getter/setter.
    Accessor {
        get: Option<Grip>,
        set: Option<Grip>,
        enumerable: bool,
        configurable: bool,
    },
}

/// Result of `prototypeAndProperties` request.
#[derive(Debug)]
pub struct PrototypeAndProperties {
    pub prototype: Grip,
    pub own_properties: BTreeMap<String, PropertyDescriptor>,
}

/// Operations on a Firefox object grip actor.
pub struct ObjectActor;

impl ObjectActor {
    /// Release the server-side object actor, freeing the associated memory.
    ///
    /// Firefox allocates a server-side actor for each object or long-string
    /// grip returned by `evaluateJSAsync`.  In long-lived daemon connections
    /// these actors accumulate and are never reclaimed.  Sending `release`
    /// to the grip actor asks Firefox to destroy it.
    ///
    /// Note: closing the underlying RDP connection also releases all actors
    /// implicitly, so calling this is only necessary on long-lived
    /// connections that outlive a single command.
    pub fn release(transport: &mut RdpTransport, actor_id: &str) -> Result<(), ProtocolError> {
        actor_request(transport, actor_id, "release", None)?;
        Ok(())
    }

    /// Fetch all properties and the prototype of an object grip.
    ///
    /// Sends `prototypeAndProperties` to the grip actor. Returns the parsed
    /// response containing `ownProperties` and `prototype`.
    pub fn prototype_and_properties(
        transport: &mut RdpTransport,
        actor_id: &str,
    ) -> Result<PrototypeAndProperties, ProtocolError> {
        let response = actor_request(transport, actor_id, "prototypeAndProperties", None)?;
        Ok(parse_prototype_and_properties(&response))
    }

    /// Fetch the names of an object's own properties.
    ///
    /// Returns the `ownPropertyNames` array from the Firefox response.
    pub fn own_property_names(
        transport: &mut RdpTransport,
        actor_id: &str,
    ) -> Result<Vec<String>, ProtocolError> {
        let response = actor_request(transport, actor_id, "ownPropertyNames", None)?;
        // Response has {"ownPropertyNames": [...], "from": "..."}
        let names = response
            .get("ownPropertyNames")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default();
        Ok(names)
    }
}

/// A grip that auto-releases its server-side actor on [`Drop`] via a queue,
/// parameterised by a [`GripKind`] marker.
///
/// Firefox allocates server-side actors for `object` and `longString` grips
/// returned by `evaluateJSAsync`.  In long-lived daemon connections these
/// actors accumulate without bound.  `GripHandle<K>` wraps the actor ID and
/// enqueues a [`ReleaseRequest`] on drop — the queue is drained by the demux
/// reader thread (daemon mode) or by the next `actor_request` call (sync CLI).
///
/// When no release queue is set (constructed via [`ScopedGrip::without_queue`]),
/// dropping the guard is a no-op and the actor leaks until the connection
/// closes.  This is acceptable for short-lived synchronous CLI connections.
///
/// Use the convenience type aliases [`ObjectScopedGrip`] and
/// [`LongStringScopedGrip`] to avoid writing the type parameter explicitly.
pub struct GripHandle<K: GripKind> {
    /// The actor ID to release on drop.  `None` for primitive grips (no actor).
    actor_id: Option<ActorId>,
    /// The full [`Grip`] value — kept for callers that need to inspect it.
    inner: Grip,
    /// Optional queue to enqueue release requests on drop.
    release_tx: Option<ReleaseQueueTx>,
    _kind: std::marker::PhantomData<K>,
}

/// Convenience alias: `GripHandle` wrapping an `ObjectGrip`.
pub type ObjectScopedGrip = GripHandle<ObjectGrip>;

/// Convenience alias: `GripHandle` wrapping a `LongStringGrip`.
pub type LongStringScopedGrip = GripHandle<LongStringGrip>;

impl<K: GripKind> GripHandle<K> {
    /// Wrap a [`Grip`] and enqueue a release on drop via `release_tx`.
    ///
    /// If `grip` is a primitive (no actor ID), the release queue is never used.
    pub fn new(grip: Grip, release_tx: ReleaseQueueTx) -> Self {
        let actor_id = match &grip {
            Grip::Object { actor, .. } | Grip::LongString { actor, .. } => Some(actor.clone()),
            _ => None,
        };
        Self {
            actor_id,
            inner: grip,
            release_tx: Some(release_tx),
            _kind: std::marker::PhantomData,
        }
    }

    /// Wrap a [`Grip`] without a release queue — actor leaks on drop.
    ///
    /// Use for short-lived synchronous CLI connections that close immediately.
    /// Equivalent to calling [`release`](Self::release) never — the connection
    /// teardown releases all actors implicitly.
    pub fn without_queue(grip: Grip) -> Self {
        let actor_id = match &grip {
            Grip::Object { actor, .. } | Grip::LongString { actor, .. } => Some(actor.clone()),
            _ => None,
        };
        Self {
            actor_id,
            inner: grip,
            release_tx: None,
            _kind: std::marker::PhantomData,
        }
    }

    /// Access the inner grip.
    pub fn grip(&self) -> &Grip {
        &self.inner
    }

    /// Consume the wrapper and release the server-side actor immediately over
    /// the transport, bypassing the release queue.
    ///
    /// For `Grip::Object` and `Grip::LongString` variants, sends the release
    /// method to the grip actor so Firefox can free the associated server-side
    /// memory immediately.  Primitive variants carry no actor and are a no-op.
    ///
    /// `unknownActor` errors from Firefox are silently swallowed.
    /// Other errors are returned to the caller.
    ///
    /// Returns the inner grip so the caller can still inspect it after release.
    ///
    /// Note: calling this disarms the drop-enqueue — the actor will not be
    /// double-released.
    pub fn release(mut self, transport: &mut RdpTransport) -> Result<Grip, ProtocolError> {
        // Disarm the drop so the queue is not also used.
        self.release_tx = None;
        let actor_id = self.actor_id.take();
        // SAFETY: we disarmed the drop above; the inner Grip is replaced with a
        // sentinel to allow the (now-disarmed) Drop to run without accessing it.
        //
        // Invariant: self.release_tx is None and self.actor_id is None at this
        // point, so the Drop impl is a no-op. We swap out inner and return the
        // original value.
        let grip = std::mem::replace(&mut self.inner, Grip::Null);
        if let Some(id) = actor_id {
            match ObjectActor::release(transport, id.as_ref()) {
                Ok(()) => {}
                Err(e) if e.is_unknown_actor() => {}
                Err(e) => return Err(e),
            }
        }
        Ok(grip)
    }

    /// Consume the wrapper, disarming the drop-enqueue without sending a
    /// release packet.  The actor leaks until connection teardown.
    ///
    /// Useful when the actor is already known to be gone (e.g. after a
    /// navigation that destroyed all actors on the page).
    pub fn disarm(mut self) -> Grip {
        self.release_tx = None;
        self.actor_id = None;
        // Swap out the inner value; the (now-disarmed) Drop is a no-op.
        std::mem::replace(&mut self.inner, Grip::Null)
    }
}

impl<K: GripKind> Drop for GripHandle<K> {
    /// Enqueue a release request on the transport release queue, if set.
    ///
    /// If the queue is full, the release is silently discarded — the actor
    /// leaks until connection teardown, which is acceptable as a graceful
    /// degradation under extreme load.
    fn drop(&mut self) {
        if let (Some(id), Some(tx)) = (self.actor_id.take(), self.release_tx.take()) {
            // Best-effort: if the queue is full or the receiver is gone, drop silently.
            let _ = tx.try_send(ReleaseRequest {
                actor_id: id,
                method: K::RELEASE_METHOD,
            });
        }
    }
}

impl<K: GripKind> std::fmt::Debug for GripHandle<K> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScopedGrip")
            .field("actor_id", &self.actor_id)
            .field("inner", &self.inner)
            .field("has_queue", &self.release_tx.is_some())
            .finish()
    }
}

/// Backward-compatible `ScopedGrip` for short-lived synchronous CLI connections.
///
/// This is the original API from before iter-76.  It wraps any grip (object,
/// long-string, or primitive) and provides an explicit [`release`] method that
/// sends the release packet immediately over the transport.  Drop does NOT
/// enqueue a release — callers must call `release` explicitly.
///
/// New code targeting the daemon should prefer [`GripHandle<K>`] with a
/// release queue for automatic cleanup on drop.
#[derive(Debug)]
pub struct ScopedGrip {
    inner: Grip,
}

impl ScopedGrip {
    /// Wrap a [`Grip`] in a scoped release wrapper.
    pub fn new(grip: Grip) -> Self {
        Self { inner: grip }
    }

    /// Access the inner grip.
    pub fn grip(&self) -> &Grip {
        &self.inner
    }

    /// Consume the wrapper and release the server-side actor.
    ///
    /// For `Grip::Object` and `Grip::LongString` variants, sends `release` to
    /// the grip actor so Firefox can free the associated server-side memory
    /// immediately.  Primitive variants (`Null`, `Undefined`, `NaN`,
    /// `Value(_)`, …) carry no actor and so are a no-op.
    ///
    /// `unknownActor` errors from Firefox are silently swallowed; the actor
    /// may already be gone if the tab was closed or the connection reset.
    /// Other actor errors and transport-level errors are returned to the
    /// caller — silently swallowing them would mask real protocol failures.
    ///
    /// Returns the inner grip so the caller can still inspect it after release.
    pub fn release(self, transport: &mut RdpTransport) -> Result<Grip, ProtocolError> {
        let actor_id: Option<&str> = match &self.inner {
            Grip::Object { actor, .. } | Grip::LongString { actor, .. } => Some(actor.as_ref()),
            _ => None,
        };
        if let Some(id) = actor_id {
            match ObjectActor::release(transport, id) {
                Ok(()) => {}
                Err(e) if e.is_unknown_actor() => {}
                Err(e) => return Err(e),
            }
        }
        Ok(self.inner)
    }
}

/// Parse a `prototypeAndProperties` response into [`PrototypeAndProperties`].
///
/// NOTE: `safeGetterValues` (computed getter results) from the response is
/// intentionally not parsed here. Non-empty values in live Firefox responses
/// will be silently dropped until a future iteration adds support.
fn parse_prototype_and_properties(response: &Value) -> PrototypeAndProperties {
    let prototype = match response.get("prototype") {
        Some(v) => Grip::from_result_value(v),
        None => Grip::Null,
    };

    let own_properties = response
        .get("ownProperties")
        .and_then(Value::as_object)
        .map(|obj| {
            obj.iter()
                .filter_map(|(key, desc)| {
                    parse_property_descriptor(desc).map(|pd| (key.clone(), pd))
                })
                .collect()
        })
        .unwrap_or_default();

    PrototypeAndProperties {
        prototype,
        own_properties,
    }
}

/// Parse a single property descriptor from the Firefox wire format.
///
/// A descriptor is treated as a data descriptor if it has `value`;
/// otherwise it is treated as an accessor descriptor, where `get` and
/// `set` are both optional. Returns `None` if `desc` is not an object.
fn parse_property_descriptor(desc: &Value) -> Option<PropertyDescriptor> {
    let obj = desc.as_object()?;

    let enumerable = obj
        .get("enumerable")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let configurable = obj
        .get("configurable")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    if let Some(value_raw) = obj.get("value") {
        // Data property.
        let writable = obj
            .get("writable")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        Some(PropertyDescriptor::Data {
            value: Grip::from_result_value(value_raw),
            writable,
            enumerable,
            configurable,
        })
    } else {
        // Accessor property — `get` and/or `set` may be present.
        let get = obj.get("get").map(Grip::from_result_value);
        let set = obj.get("set").map(Grip::from_result_value);

        Some(PropertyDescriptor::Accessor {
            get,
            set,
            enumerable,
            configurable,
        })
    }
}

/// Convert a [`PropertyDescriptor`] to a JSON value for CLI output.
pub fn descriptor_to_json(desc: &PropertyDescriptor) -> Value {
    match desc {
        PropertyDescriptor::Data {
            value,
            writable,
            enumerable,
            configurable,
        } => json!({
            "writable": writable,
            "enumerable": enumerable,
            "configurable": configurable,
            "value": value.to_json(),
        }),
        PropertyDescriptor::Accessor {
            get,
            set,
            enumerable,
            configurable,
        } => {
            let mut obj = json!({
                "enumerable": enumerable,
                "configurable": configurable,
            });
            if let Some(g) = get {
                obj["get"] = g.to_json();
            }
            if let Some(s) = set {
                obj["set"] = s.to_json();
            }
            obj
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    // --- parse_property_descriptor tests ---

    #[test]
    fn parse_data_descriptor_full() {
        let desc = json!({
            "value": 42,
            "writable": true,
            "enumerable": true,
            "configurable": false
        });
        let pd = parse_property_descriptor(&desc).unwrap();
        match pd {
            PropertyDescriptor::Data {
                value,
                writable,
                enumerable,
                configurable,
            } => {
                assert_eq!(value, Grip::Value(json!(42)));
                assert!(writable);
                assert!(enumerable);
                assert!(!configurable);
            }
            other @ PropertyDescriptor::Accessor { .. } => {
                panic!("expected Data, got {other:?}")
            }
        }
    }

    #[test]
    fn parse_data_descriptor_with_object_value() {
        let desc = json!({
            "value": {
                "type": "object",
                "actor": "server1.conn0.child2/obj5",
                "class": "Array"
            },
            "writable": false,
            "enumerable": true,
            "configurable": true
        });
        let pd = parse_property_descriptor(&desc).unwrap();
        let PropertyDescriptor::Data { value, .. } = pd else {
            panic!("expected Data variant");
        };
        let Grip::Object { class, .. } = value else {
            panic!("expected Grip::Object");
        };
        assert_eq!(class, "Array");
    }

    #[test]
    fn parse_data_descriptor_missing_flags_defaults_to_false() {
        // Firefox sometimes omits boolean flags for non-enumerable properties.
        let desc = json!({"value": "hello"});
        let pd = parse_property_descriptor(&desc).unwrap();
        match pd {
            PropertyDescriptor::Data {
                value,
                writable,
                enumerable,
                configurable,
            } => {
                assert_eq!(value, Grip::Value(json!("hello")));
                assert!(!writable);
                assert!(!enumerable);
                assert!(!configurable);
            }
            other @ PropertyDescriptor::Accessor { .. } => {
                panic!("expected Data, got {other:?}")
            }
        }
    }

    #[test]
    fn parse_accessor_descriptor_with_getter() {
        let desc = json!({
            "get": {
                "type": "object",
                "actor": "server1.conn0.child2/obj10",
                "class": "Function"
            },
            "set": {"type": "undefined"},
            "enumerable": false,
            "configurable": true
        });
        let pd = parse_property_descriptor(&desc).unwrap();
        match pd {
            PropertyDescriptor::Accessor {
                get,
                set,
                enumerable,
                configurable,
            } => {
                assert!(get.is_some());
                assert_eq!(set, Some(Grip::Undefined));
                assert!(!enumerable);
                assert!(configurable);
            }
            other @ PropertyDescriptor::Data { .. } => {
                panic!("expected Accessor, got {other:?}")
            }
        }
    }

    #[test]
    fn parse_accessor_descriptor_empty() {
        // A bare accessor with neither get nor set (unusual but valid wire format).
        let desc = json!({"enumerable": true, "configurable": true});
        let pd = parse_property_descriptor(&desc).unwrap();
        match pd {
            PropertyDescriptor::Accessor { get, set, .. } => {
                assert!(get.is_none());
                assert!(set.is_none());
            }
            other @ PropertyDescriptor::Data { .. } => {
                panic!("expected Accessor, got {other:?}")
            }
        }
    }

    #[test]
    fn parse_descriptor_returns_none_for_non_object() {
        assert!(parse_property_descriptor(&json!("string")).is_none());
        assert!(parse_property_descriptor(&json!(null)).is_none());
        assert!(parse_property_descriptor(&json!(42)).is_none());
    }

    // --- parse_prototype_and_properties tests ---

    #[test]
    fn parse_prototype_and_properties_typical() {
        let response = json!({
            "from": "server1.conn0.child2/obj19",
            "prototype": {
                "type": "object",
                "actor": "server1.conn0.child2/obj20",
                "class": "Object"
            },
            "ownProperties": {
                "a": {
                    "value": 1,
                    "writable": true,
                    "enumerable": true,
                    "configurable": true
                },
                "b": {
                    "value": {"type": "object", "actor": "server1.conn0.child2/obj21", "class": "Array"},
                    "writable": true,
                    "enumerable": true,
                    "configurable": true
                }
            }
        });

        let pap = parse_prototype_and_properties(&response);
        let Grip::Object { class, .. } = &pap.prototype else {
            panic!("expected Object grip for prototype");
        };
        assert_eq!(class, "Object");
        assert_eq!(pap.own_properties.len(), 2);
        assert!(pap.own_properties.contains_key("a"));
        assert!(pap.own_properties.contains_key("b"));
    }

    #[test]
    fn parse_prototype_and_properties_empty() {
        let response = json!({
            "from": "server1.conn0.child2/obj1",
            "prototype": {"type": "null"},
            "ownProperties": {}
        });
        let pap = parse_prototype_and_properties(&response);
        assert_eq!(pap.prototype, Grip::Null);
        assert!(pap.own_properties.is_empty());
    }

    #[test]
    fn parse_prototype_and_properties_missing_fields() {
        // Minimal response — should not panic.
        // When `prototype` is absent, we return `Grip::Null` (sentinel for a
        // missing prototype rather than a Firefox-typed null discriminator).
        let response = json!({"from": "server1.conn0.child2/obj1"});
        let pap = parse_prototype_and_properties(&response);
        assert_eq!(pap.prototype, Grip::Null);
        assert!(pap.own_properties.is_empty());
    }

    // --- descriptor_to_json tests ---

    #[test]
    fn descriptor_to_json_data() {
        let desc = PropertyDescriptor::Data {
            value: Grip::Value(json!(99)),
            writable: true,
            enumerable: true,
            configurable: false,
        };
        let j = descriptor_to_json(&desc);
        assert_eq!(j["value"], 99);
        assert_eq!(j["writable"], true);
        assert_eq!(j["enumerable"], true);
        assert_eq!(j["configurable"], false);
    }

    #[test]
    fn descriptor_to_json_accessor_with_getter() {
        let desc = PropertyDescriptor::Accessor {
            get: Some(Grip::Object {
                actor: "obj1".into(),
                class: "Function".to_owned(),
                preview: None,
            }),
            set: None,
            enumerable: false,
            configurable: true,
        };
        let j = descriptor_to_json(&desc);
        assert_eq!(j["enumerable"], false);
        assert_eq!(j["configurable"], true);
        assert_eq!(j["get"]["type"], "object");
        assert!(j.get("set").is_none());
    }

    // --- GripHandle<K> (generic, iter-76) ---

    /// AC: `grip_drop_enqueues_release` — dropping a `ScopedGrip<ObjectGrip>`
    /// adds a release entry to the transport queue with the correct actor ID and
    /// method name.
    #[test]
    fn grip_drop_enqueues_release() {
        let (tx, rx) = release_queue(16);

        let grip = Grip::Object {
            actor: "conn0/obj42".into(),
            class: "Array".to_owned(),
            preview: None,
        };
        let scoped: GripHandle<ObjectGrip> = GripHandle::new(grip, tx);

        // Drop it — should enqueue a release request.
        drop(scoped);

        let req = rx.try_recv().expect("release request must be enqueued");
        assert_eq!(req.actor_id, "conn0/obj42");
        assert_eq!(req.method, "release");
    }

    #[test]
    fn grip_drop_without_queue_is_noop() {
        // Dropping without a queue must not panic.
        let grip = Grip::Object {
            actor: "conn0/obj1".into(),
            class: "Object".to_owned(),
            preview: None,
        };
        let scoped: GripHandle<ObjectGrip> = GripHandle::without_queue(grip);
        drop(scoped); // no panic, no enqueue
    }

    #[test]
    fn grip_primitive_does_not_enqueue() {
        let (tx, rx) = release_queue(16);
        let scoped: GripHandle<ObjectGrip> = GripHandle::new(Grip::Null, tx);
        drop(scoped);
        // Null grip has no actor, nothing should arrive.
        assert!(
            rx.try_recv().is_err(),
            "primitive grip must not enqueue a release"
        );
    }

    #[test]
    fn grip_disarm_does_not_enqueue() {
        let (tx, rx) = release_queue(16);
        let grip = Grip::Object {
            actor: "conn0/obj7".into(),
            class: "Function".to_owned(),
            preview: None,
        };
        let scoped: GripHandle<ObjectGrip> = GripHandle::new(grip, tx);
        scoped.disarm(); // should not enqueue
        assert!(rx.try_recv().is_err(), "disarmed grip must not enqueue");
    }

    #[test]
    fn long_string_grip_enqueues_release_with_correct_method() {
        let (tx, rx) = release_queue(16);
        let grip = Grip::LongString {
            actor: "conn0/longStr3".into(),
            initial: "hello world".to_owned(),
            length: 11,
        };
        let scoped: GripHandle<LongStringGrip> = GripHandle::new(grip, tx);
        drop(scoped);

        let req = rx.try_recv().expect("release request must be enqueued");
        assert_eq!(req.actor_id, "conn0/longStr3");
        assert_eq!(req.method, "release");
    }
}
