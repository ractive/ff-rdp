//! Shared protocol value types used across multiple actor specs.
//!
//! These types handle Firefox's polymorphic wire shapes that appear in many
//! actors (e.g. `longString` vs inline string values).

use serde::de::{self, Deserializer, MapAccess, Visitor};
use serde::ser::{SerializeMap, Serializer};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::actors::string::LongStringActor;
use crate::error::ProtocolError;
use crate::transport::RdpTransport;
use crate::types::ActorId;

// ---------------------------------------------------------------------------
// LongString
// ---------------------------------------------------------------------------

/// A Firefox value that is either an inline string or a long-string actor reference.
///
/// Firefox sends small strings inline as a bare JSON string.  Strings exceeding
/// the inline threshold (~10 KB) are sent as:
/// ```json
/// { "type": "longString", "actor": "conn0/longString1", "length": 50000, "initial": "..." }
/// ```
///
/// Use [`LongString::fetch_full`] to get the complete string for the actor variant.
/// The actor variant carries the first ~1 KB in `initial` which is often sufficient
/// for display purposes without a round-trip.
///
/// # Serialization
///
/// `Inline` serializes as a bare JSON string.  Serializing the `Actor` variant
/// re-emits the full `longString` object shape; this is primarily useful for
/// debugging and round-tripping — Firefox itself only ever sends longString
/// objects to clients and never accepts them as input.
#[derive(Debug, Clone)]
pub enum LongString {
    /// A small string sent inline on the wire.
    Inline(String),
    /// A large string referenced by an actor ID; fetch via [`LongString::fetch_full`].
    Actor {
        actor: ActorId,
        length: u64,
        initial: String,
    },
}

/// Maximum number of bytes that [`LongString::fetch_full`] will allocate.
///
/// Server-provided `length` values larger than this are rejected with
/// [`ProtocolError::InvalidPacket`] before any allocation is made, guarding
/// against memory-exhaustion from a malicious or misbehaving Firefox server.
pub const LONGSTRING_MAX_FETCH_BYTES: usize = 16 * 1024 * 1024; // 16 MiB

impl LongString {
    /// Return a best-effort display value without a network round-trip.
    ///
    /// For `Inline` this is the full string.  For `Actor` this is the `initial`
    /// prefix only — callers should append a truncation indicator (e.g. `"…"`)
    /// if they need to signal that the string continues beyond this segment.
    pub fn preview(&self) -> &str {
        match self {
            Self::Inline(s) => s.as_str(),
            Self::Actor { initial, .. } => initial.as_str(),
        }
    }

    /// Fetch the complete string content, issuing `longstring.substring` calls
    /// as needed for the `Actor` variant.
    ///
    /// For `Inline` this is a zero-copy borrow-and-clone.  For `Actor` this
    /// performs one or more round-trips to the Firefox server.
    ///
    /// Returns [`ProtocolError::InvalidPacket`] if `length` exceeds
    /// [`LONGSTRING_MAX_FETCH_BYTES`] (16 MiB) to prevent unbounded allocation.
    pub fn fetch_full(&self, transport: &mut RdpTransport) -> Result<String, ProtocolError> {
        match self {
            Self::Inline(s) => Ok(s.clone()),
            Self::Actor { actor, length, .. } => {
                let len = usize::try_from(*length).unwrap_or(usize::MAX);
                if len > LONGSTRING_MAX_FETCH_BYTES {
                    return Err(ProtocolError::InvalidPacket(format!(
                        "longString actor {} declared length {} exceeds max {} bytes",
                        actor.as_ref(),
                        length,
                        LONGSTRING_MAX_FETCH_BYTES,
                    )));
                }
                LongStringActor::full_string(transport, actor.as_ref(), *length)
            }
        }
    }

    /// Return `true` when the value is an actor reference (large string).
    pub fn is_actor(&self) -> bool {
        matches!(self, Self::Actor { .. })
    }
}

impl Default for LongString {
    fn default() -> Self {
        Self::Inline(String::new())
    }
}

/// Resolve a JSON value slot that Firefox declares `longstring` into its full
/// string content, fetching from the long-string actor when the slot arrived as
/// a grip rather than an inline string.
///
/// Firefox sends values in a `longstring` spec slot inline as a bare string when
/// they are below the `DebuggerServer.LONG_STRING_LENGTH` threshold (~10 KB), and
/// as a `{type:"longString", actor, length, initial}` grip when they exceed it.
/// A bare `Value::as_str()` therefore returns `None` for the grip form, silently
/// dropping large values.  This helper handles both shapes uniformly:
///
/// - absent slot or JSON `null` → `Ok(None)`
/// - inline string → `Ok(Some(string))` (no round-trip)
/// - `longString` grip → one or more `substring` round-trips via
///   [`LongString::fetch_full`], returning the complete value
///
/// Any other JSON shape (number, bool, object without `type:"longString"`)
/// returns [`ProtocolError::InvalidPacket`] so protocol drift surfaces loudly
/// instead of being silently coerced to empty.
pub fn resolve_long_string_slot(
    transport: &mut RdpTransport,
    slot: Option<&Value>,
) -> Result<Option<String>, ProtocolError> {
    match slot {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(value @ Value::Object(_)) => {
            let ls: LongString = serde_json::from_value(value.clone()).map_err(|e| {
                ProtocolError::InvalidPacket(format!(
                    "longstring slot is an object but not a longString grip: {e}"
                ))
            })?;
            ls.fetch_full(transport).map(Some)
        }
        Some(other) => Err(ProtocolError::InvalidPacket(format!(
            "longstring slot has unexpected JSON shape: {other}"
        ))),
    }
}

// ---------------------------------------------------------------------------
// Custom Deserialize — handles both wire shapes
// ---------------------------------------------------------------------------

impl<'de> Deserialize<'de> for LongString {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct LongStringVisitor;

        impl<'de> Visitor<'de> for LongStringVisitor {
            type Value = LongString;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a string or a longString object")
            }

            // Bare JSON string → Inline
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(LongString::Inline(v.to_owned()))
            }

            fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
                Ok(LongString::Inline(v))
            }

            // JSON object → actor variant (must have type="longString")
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut kind: Option<String> = None;
                let mut actor: Option<String> = None;
                let mut length: Option<u64> = None;
                let mut initial: Option<String> = None;

                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "type" => kind = Some(map.next_value()?),
                        "actor" => actor = Some(map.next_value()?),
                        "length" => length = Some(map.next_value()?),
                        "initial" => initial = Some(map.next_value()?),
                        _ => {
                            map.next_value::<de::IgnoredAny>()?;
                        }
                    }
                }

                match kind.as_deref() {
                    Some("longString") => {
                        let actor = actor.ok_or_else(|| de::Error::missing_field("actor"))?;
                        let length = length.ok_or_else(|| de::Error::missing_field("length"))?;
                        let initial = initial.unwrap_or_default();
                        Ok(LongString::Actor {
                            actor: ActorId::from(actor.as_str()),
                            length,
                            initial,
                        })
                    }
                    Some(other) => Err(de::Error::custom(format!(
                        "unknown longString type: {other}"
                    ))),
                    None => Err(de::Error::missing_field("type")),
                }
            }
        }

        deserializer.deserialize_any(LongStringVisitor)
    }
}

// ---------------------------------------------------------------------------
// Custom Serialize — Inline → bare string; Actor → compact object
// ---------------------------------------------------------------------------

impl Serialize for LongString {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Inline(s) => serializer.serialize_str(s),
            Self::Actor {
                actor,
                length,
                initial,
            } => {
                let mut map = serializer.serialize_map(Some(4))?;
                map.serialize_entry("type", "longString")?;
                map.serialize_entry("actor", actor.as_ref())?;
                map.serialize_entry("length", length)?;
                map.serialize_entry("initial", initial)?;
                map.end()
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn inline_deserializes_from_bare_string() {
        let v = json!("hello world");
        let ls: LongString = serde_json::from_value(v).unwrap();
        assert!(matches!(ls, LongString::Inline(s) if s == "hello world"));
    }

    #[test]
    fn actor_deserializes_from_longstring_object() {
        let v = json!({
            "type": "longString",
            "actor": "conn0/longString1",
            "length": 50000,
            "initial": "the first kilobyte..."
        });
        let ls: LongString = serde_json::from_value(v).unwrap();
        match ls {
            LongString::Actor {
                actor,
                length,
                initial,
            } => {
                assert_eq!(actor.as_ref(), "conn0/longString1");
                assert_eq!(length, 50000);
                assert_eq!(initial, "the first kilobyte...");
            }
            LongString::Inline(_) => panic!("expected Actor variant"),
        }
    }

    #[test]
    fn actor_without_initial_defaults_to_empty() {
        let v = json!({
            "type": "longString",
            "actor": "conn0/longString2",
            "length": 1024
        });
        let ls: LongString = serde_json::from_value(v).unwrap();
        match ls {
            LongString::Actor { initial, .. } => assert_eq!(initial, ""),
            LongString::Inline(_) => panic!("expected Actor variant"),
        }
    }

    #[test]
    fn inline_serializes_as_bare_string() {
        let ls = LongString::Inline("hello".to_owned());
        let v = serde_json::to_value(&ls).unwrap();
        assert_eq!(v, json!("hello"));
    }

    #[test]
    fn actor_serializes_as_longstring_object() {
        let ls = LongString::Actor {
            actor: ActorId::from("conn0/ls1"),
            length: 99_999,
            initial: "prefix".to_owned(),
        };
        let v = serde_json::to_value(&ls).unwrap();
        assert_eq!(v["type"], "longString");
        assert_eq!(v["actor"], "conn0/ls1");
        assert_eq!(v["length"], 99_999);
        assert_eq!(v["initial"], "prefix");
    }

    #[test]
    fn preview_inline_returns_full_string() {
        let ls = LongString::Inline("full content".to_owned());
        assert_eq!(ls.preview(), "full content");
    }

    #[test]
    fn preview_actor_returns_initial() {
        let ls = LongString::Actor {
            actor: ActorId::from("conn0/ls1"),
            length: 50_000,
            initial: "first 1kb".to_owned(),
        };
        assert_eq!(ls.preview(), "first 1kb");
    }

    #[test]
    fn default_is_inline_empty_string() {
        let ls = LongString::default();
        assert!(matches!(ls, LongString::Inline(s) if s.is_empty()));
    }

    #[test]
    fn is_actor_distinguishes_variants() {
        assert!(!LongString::Inline("x".to_owned()).is_actor());
        assert!(
            LongString::Actor {
                actor: ActorId::from("a"),
                length: 1,
                initial: String::new()
            }
            .is_actor()
        );
    }

    #[test]
    fn deserialize_error_on_unknown_type() {
        let v = json!({"type": "unknownType", "actor": "x", "length": 1});
        let err = serde_json::from_value::<LongString>(v);
        assert!(err.is_err());
    }

    #[test]
    fn roundtrip_inline() {
        let original = LongString::Inline("round trip content".to_owned());
        let v = serde_json::to_value(&original).unwrap();
        let restored: LongString = serde_json::from_value(v).unwrap();
        assert!(matches!(restored, LongString::Inline(s) if s == "round trip content"));
    }

    // -----------------------------------------------------------------------
    // resolve_long_string_slot (iter-102 longString sweep)
    // -----------------------------------------------------------------------

    /// A transport backed by a loopback TCP pair.  Slots that don't require a
    /// fetch (absent/null/inline) never touch the socket.
    fn dummy_transport() -> RdpTransport {
        use std::io::BufReader;
        use std::net::{TcpListener, TcpStream};
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let client = TcpStream::connect(addr).unwrap();
        let (_server, _) = listener.accept().unwrap();
        let writer = client.try_clone().unwrap();
        let reader = BufReader::new(client);
        RdpTransport::from_parts(reader, writer)
    }

    #[test]
    fn resolve_slot_absent_is_none() {
        let mut t = dummy_transport();
        assert_eq!(resolve_long_string_slot(&mut t, None).unwrap(), None);
    }

    #[test]
    fn resolve_slot_null_is_none() {
        let mut t = dummy_transport();
        let v = Value::Null;
        assert_eq!(resolve_long_string_slot(&mut t, Some(&v)).unwrap(), None);
    }

    #[test]
    fn resolve_slot_inline_returns_string_without_roundtrip() {
        let mut t = dummy_transport();
        let v = json!("inline value");
        assert_eq!(
            resolve_long_string_slot(&mut t, Some(&v)).unwrap(),
            Some("inline value".to_owned())
        );
    }

    #[test]
    fn resolve_slot_non_string_scalar_errors() {
        let mut t = dummy_transport();
        let v = json!(42);
        let err = resolve_long_string_slot(&mut t, Some(&v)).unwrap_err();
        assert!(matches!(err, ProtocolError::InvalidPacket(_)));
    }

    #[test]
    fn resolve_slot_object_without_longstring_type_errors() {
        let mut t = dummy_transport();
        let v = json!({ "not": "a grip" });
        let err = resolve_long_string_slot(&mut t, Some(&v)).unwrap_err();
        assert!(matches!(err, ProtocolError::InvalidPacket(_)));
    }

    /// A `longString` grip slot fetches the full value via `substring`.  A mock
    /// server answers the single `substring` request with the full content.
    #[test]
    fn resolve_slot_longstring_grip_fetches_full_value() {
        use std::io::{BufReader, Write};
        use std::net::TcpListener;
        use std::time::Duration;

        use crate::transport::{encode_frame, recv_from};

        let full = "Z".repeat(20_000);
        let full_for_server = full.clone();

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let handle = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut writer = stream.try_clone().unwrap();
            let mut reader = BufReader::new(stream);

            // Greeting (consumed by RdpTransport::connect).
            let greeting = json!({"from":"root","applicationType":"browser","traits":{}});
            writer
                .write_all(encode_frame(&serde_json::to_string(&greeting).unwrap()).as_bytes())
                .unwrap();

            // Read one substring request and reply with the full content.
            let req = recv_from(&mut reader).unwrap();
            assert_eq!(req["type"], "substring");
            assert_eq!(req["to"], "conn0/longString7");
            let resp = json!({"from":"conn0/longString7","substring": full_for_server});
            writer
                .write_all(encode_frame(&serde_json::to_string(&resp).unwrap()).as_bytes())
                .unwrap();
        });

        let mut transport =
            RdpTransport::connect("127.0.0.1", port, Duration::from_secs(5)).unwrap();

        let grip = json!({
            "type": "longString",
            "actor": "conn0/longString7",
            "length": 20_000,
            "initial": "Z".repeat(1024),
        });
        let resolved = resolve_long_string_slot(&mut transport, Some(&grip))
            .unwrap()
            .expect("grip must resolve to Some");
        assert_eq!(resolved.len(), 20_000);
        assert_eq!(resolved, full);

        handle.join().unwrap();
    }
}
