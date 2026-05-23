//! Shared protocol value types used across multiple actor specs.
//!
//! These types handle Firefox's polymorphic wire shapes that appear in many
//! actors (e.g. `longString` vs inline string values).

use serde::de::{self, Deserializer, MapAccess, Visitor};
use serde::ser::{SerializeMap, Serializer};
use serde::{Deserialize, Serialize};

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
/// `Inline` serializes as a bare JSON string.  The `Actor` variant is not
/// re-emitted by this crate (it is a received-only shape), so its serialization
/// produces a compact object — callers typically call [`LongString::fetch_full`]
/// and work with the resolved `String`.
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

impl LongString {
    /// Return a best-effort display value without a network round-trip.
    ///
    /// For `Inline` this is the full string.  For `Actor` this is the `initial`
    /// prefix followed by `"…"` when the string is longer than the initial segment.
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
    pub fn fetch_full(&self, transport: &mut RdpTransport) -> Result<String, ProtocolError> {
        match self {
            Self::Inline(s) => Ok(s.clone()),
            Self::Actor { actor, length, .. } => {
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
}
