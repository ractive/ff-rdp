use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::actor::actor_request;
use crate::error::ProtocolError;
use crate::transport::RdpTransport;
use crate::types::{ActorId, Grip};

// ---------------------------------------------------------------------------
// GripKind marker trait and concrete markers
// ---------------------------------------------------------------------------

/// Marker trait for the kind of server-side actor a [`GripHandle`] wraps.
///
/// The marker determines which release method to send to Firefox:
/// [`LongStringGrip`] → `"release"` (per
/// `devtools/server/actors/string.js:40-50`). The marker type exists to
/// enforce type-safety at call sites (you cannot accidentally release a
/// long-string via an object actor's release method, should another grip kind
/// with a different release method be added later).
///
/// The `ObjectGrip` marker variant from iter-76 was removed in the
/// reset/structure-core collapse: once the release-queue machinery below it
/// (`release_queue`/`ReleaseQueueTx`/`ReleaseRequest`, also removed — see that
/// commit) had no non-test consumer, `GripHandle<ObjectGrip>` and its
/// `ObjectScopedGrip` alias had none either. The synchronous [`ScopedGrip`]
/// below is what `ff-rdp-cli`'s `eval` command actually uses for object grips.
pub trait GripKind: sealed::Sealed {
    /// The Firefox RDP method name to invoke when releasing this kind.
    const RELEASE_METHOD: &'static str;
}

mod sealed {
    pub trait Sealed {}
}

/// Marker: the wrapped actor is a long-string grip.
pub struct LongStringGrip;

impl sealed::Sealed for LongStringGrip {}
impl GripKind for LongStringGrip {
    const RELEASE_METHOD: &'static str = "release";
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
    /// grip returned by `evaluateJSAsync`.  On a long-lived connection these
    /// actors accumulate and are never reclaimed.  Sending `release`
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
}

/// A grip that auto-releases its server-side actor on [`Drop`] via a queue,
/// parameterised by a [`GripKind`] marker.
///
/// Firefox allocates server-side actors for `object` and `longString` grips
/// returned by `evaluateJSAsync`.  `GripHandle<K>` wraps the actor ID and
/// exposes [`release`](Self::release) to free it immediately over the
/// transport.
///
/// A queued, release-on-drop variant existed through iter-76 but had no
/// non-test consumer once the daemon's long-lived-connection use case went
/// away (see `kb/decision-log.md` DEC-056) and was removed in the
/// reset/structure-core collapse, along with the `ObjectGrip` marker and its
/// `ObjectScopedGrip` alias. [`without_queue`](Self::without_queue) is the
/// only constructor; the name is kept (rather than renaming to e.g. `new`) so
/// the sole production call site —
/// `LongStringScopedGrip::without_queue(grip).release(transport)` in
/// `ff-rdp-cli`'s `navigate` command — did not need to change.
pub struct GripHandle<K: GripKind> {
    /// The actor ID to release.  `None` for primitive grips (no actor).
    actor_id: Option<ActorId>,
    /// The full [`Grip`] value, consumed by [`release`](Self::release).
    inner: Grip,
    _kind: std::marker::PhantomData<K>,
}

/// Convenience alias: `GripHandle` wrapping a `LongStringGrip`.
pub type LongStringScopedGrip = GripHandle<LongStringGrip>;

impl<K: GripKind> GripHandle<K> {
    /// Wrap a [`Grip`]. The actor leaks until connection teardown unless
    /// [`release`](Self::release) is called explicitly.
    ///
    /// Use for short-lived synchronous CLI connections that close immediately.
    pub fn without_queue(grip: Grip) -> Self {
        let actor_id = match &grip {
            Grip::Object { actor, .. } | Grip::LongString { actor, .. } => Some(actor.clone()),
            _ => None,
        };
        Self {
            actor_id,
            inner: grip,
            _kind: std::marker::PhantomData,
        }
    }

    /// Consume the wrapper and release the server-side actor immediately over
    /// the transport.
    ///
    /// For `Grip::Object` and `Grip::LongString` variants, sends the release
    /// method to the grip actor so Firefox can free the associated server-side
    /// memory immediately.  Primitive variants carry no actor and are a no-op.
    ///
    /// `unknownActor` errors from Firefox are silently swallowed.
    /// Other errors are returned to the caller.
    ///
    /// Returns the inner grip so the caller can still inspect it after release.
    pub fn release(self, transport: &mut RdpTransport) -> Result<Grip, ProtocolError> {
        if let Some(id) = &self.actor_id {
            match ObjectActor::release(transport, id.as_ref()) {
                Ok(()) => {}
                Err(e) if e.is_unknown_actor() => {}
                Err(e) => return Err(e),
            }
        }
        Ok(self.inner)
    }
}

impl<K: GripKind> std::fmt::Debug for GripHandle<K> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GripHandle")
            .field("actor_id", &self.actor_id)
            .field("inner", &self.inner)
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
/// `GripHandle<K>`'s release-queue variant (automatic cleanup on drop) was
/// removed in the reset/structure-core collapse for having no non-test
/// consumer; `GripHandle::without_queue` + explicit `.release()` is now
/// equivalent to this type for the one grip kind (`LongStringGrip`) that still
/// uses it.
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

    // --- GripHandle<LongStringGrip> (`LongStringScopedGrip`) ---

    fn make_transport_pair() -> (RdpTransport, std::net::TcpStream) {
        use std::io::BufReader;
        use std::net::{TcpListener, TcpStream};

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let client = TcpStream::connect(addr).unwrap();
        let (server, _) = listener.accept().unwrap();
        let writer = client.try_clone().unwrap();
        let reader = BufReader::new(client);
        (RdpTransport::from_parts(reader, writer), server)
    }

    /// AC: `grip_release_sends_release_request` — `without_queue(grip).release(transport)`
    /// sends `release` to the grip's actor and surfaces `unknownActor` as a
    /// silent no-op (mirrors `ScopedGrip::release`'s swallow behaviour, see
    /// `commands/navigate.rs`'s LongString header fetch-back path).
    #[test]
    fn grip_release_sends_release_request() {
        use std::io::{BufReader, Write as _};

        let (mut transport, server) = make_transport_pair();
        let grip = Grip::LongString {
            actor: "conn0/longStr3".into(),
            initial: "hello world".to_owned(),
            length: 11,
        };
        let scoped: LongStringScopedGrip = GripHandle::without_queue(grip);

        let t = std::thread::spawn(move || {
            let mut reader = BufReader::new(server.try_clone().unwrap());
            let req = crate::transport::recv_from(&mut reader).unwrap();
            assert_eq!(req["to"], "conn0/longStr3");
            assert_eq!(req["type"], "release");
            let reply = serde_json::json!({"from": "conn0/longStr3"});
            let frame = crate::transport::encode_frame(&reply.to_string());
            let mut s = &server;
            s.write_all(frame.as_bytes()).unwrap();
        });

        let returned = scoped.release(&mut transport).expect("release");
        assert_eq!(
            returned,
            Grip::LongString {
                actor: "conn0/longStr3".into(),
                initial: "hello world".to_owned(),
                length: 11,
            }
        );
        t.join().unwrap();
    }

    /// Primitive grips carry no actor, so `release` must not touch the
    /// transport at all.
    #[test]
    fn grip_release_primitive_is_noop() {
        let (mut transport, _server) = make_transport_pair();
        let scoped: LongStringScopedGrip = GripHandle::without_queue(Grip::Null);
        let returned = scoped.release(&mut transport).expect("release");
        assert_eq!(returned, Grip::Null);
    }
}
