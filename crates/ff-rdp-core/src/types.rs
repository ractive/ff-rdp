use std::fmt;

use serde::Serialize;
use serde_json::Value;

/// A newtype wrapping a Firefox RDP actor ID string.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct ActorId(String);

impl fmt::Display for ActorId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for ActorId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for ActorId {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

impl AsRef<str> for ActorId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// Represents a Firefox RDP "grip" — a reference to a remote JS value or object.
///
/// Firefox encodes eval results with a `type` discriminator for values that
/// cannot be represented as plain JSON (e.g. `undefined`, `Infinity`, objects).
#[derive(Debug, Clone, PartialEq)]
pub enum Grip {
    Null,
    Undefined,
    Inf,
    NegInf,
    NaN,
    NegZero,
    LongString {
        actor: ActorId,
        length: u64,
        initial: String,
    },
    Object {
        actor: ActorId,
        class: String,
        preview: Option<Value>,
    },
    /// Plain JSON values: string, number, bool, array, or object without a
    /// Firefox-specific `type` discriminator.
    Value(Value),
}

impl Grip {
    /// Parse a Firefox RDP result value into a [`Grip`].
    ///
    /// The `value` argument must be the content of the `"result"` field in a
    /// Firefox RDP eval response — not the outer response object itself.
    ///
    /// # Examples
    ///
    /// ```
    /// use ff_rdp_core::types::Grip;
    /// use serde_json::json;
    ///
    /// assert_eq!(Grip::from_result_value(&json!({"type": "undefined"})), Grip::Undefined);
    /// assert_eq!(Grip::from_result_value(&json!(42)), Grip::Value(json!(42)));
    /// ```
    pub fn from_result_value(value: &Value) -> Self {
        // If the result is a JSON object with a "type" field, Firefox is
        // signalling a special grip kind.
        if let Some(obj) = value.as_object()
            && let Some(type_str) = obj.get("type").and_then(Value::as_str)
        {
            match type_str {
                "null" => return Self::Null,
                "undefined" => return Self::Undefined,
                "Infinity" => return Self::Inf,
                "-Infinity" => return Self::NegInf,
                "NaN" => return Self::NaN,
                "-0" => return Self::NegZero,
                "longString" => {
                    if let Some(actor) = obj.get("actor").and_then(Value::as_str) {
                        let length = obj
                            .get("length")
                            .and_then(Value::as_u64)
                            .unwrap_or_default();
                        let initial = obj
                            .get("initial")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned();
                        return Self::LongString {
                            actor: actor.into(),
                            length,
                            initial,
                        };
                    }
                }
                "object" => {
                    if let (Some(actor), Some(class)) = (
                        obj.get("actor").and_then(Value::as_str),
                        obj.get("class").and_then(Value::as_str),
                    ) {
                        let preview = obj.get("preview").cloned();
                        return Self::Object {
                            actor: actor.into(),
                            class: class.to_owned(),
                            preview,
                        };
                    }
                }
                _ => {}
            }
        }

        // Everything else — plain JSON scalars, arrays, objects — is a Value.
        Self::Value(value.clone())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn actor_id_display() {
        let id = ActorId::from("server1.conn0.child1/tab1");
        assert_eq!(id.to_string(), "server1.conn0.child1/tab1");
    }

    #[test]
    fn actor_id_as_ref() {
        let id = ActorId::from("root");
        assert_eq!(id.as_ref(), "root");
    }

    #[test]
    fn grip_undefined() {
        assert_eq!(
            Grip::from_result_value(&json!({"type": "undefined"})),
            Grip::Undefined
        );
    }

    #[test]
    fn grip_null() {
        assert_eq!(
            Grip::from_result_value(&json!({"type": "null"})),
            Grip::Null
        );
    }

    #[test]
    fn grip_infinity() {
        assert_eq!(
            Grip::from_result_value(&json!({"type": "Infinity"})),
            Grip::Inf
        );
    }

    #[test]
    fn grip_neg_infinity() {
        assert_eq!(
            Grip::from_result_value(&json!({"type": "-Infinity"})),
            Grip::NegInf
        );
    }

    #[test]
    fn grip_nan() {
        assert_eq!(Grip::from_result_value(&json!({"type": "NaN"})), Grip::NaN);
    }

    #[test]
    fn grip_neg_zero() {
        assert_eq!(
            Grip::from_result_value(&json!({"type": "-0"})),
            Grip::NegZero
        );
    }

    #[test]
    fn grip_long_string() {
        let v = json!({
            "type": "longString",
            "actor": "server1.conn0.child1/longString1",
            "length": 100_000,
            "initial": "hello"
        });
        assert_eq!(
            Grip::from_result_value(&v),
            Grip::LongString {
                actor: ActorId::from("server1.conn0.child1/longString1"),
                length: 100_000,
                initial: "hello".to_owned(),
            }
        );
    }

    #[test]
    fn grip_object() {
        let v = json!({
            "type": "object",
            "actor": "server1.conn0.child1/obj1",
            "class": "Array",
            "preview": {"length": 3}
        });
        assert_eq!(
            Grip::from_result_value(&v),
            Grip::Object {
                actor: ActorId::from("server1.conn0.child1/obj1"),
                class: "Array".to_owned(),
                preview: Some(json!({"length": 3})),
            }
        );
    }

    #[test]
    fn grip_plain_string() {
        assert_eq!(
            Grip::from_result_value(&json!("hello")),
            Grip::Value(json!("hello"))
        );
    }

    #[test]
    fn grip_number() {
        assert_eq!(Grip::from_result_value(&json!(42)), Grip::Value(json!(42)));
    }

    #[test]
    fn grip_bool() {
        assert_eq!(
            Grip::from_result_value(&json!(true)),
            Grip::Value(json!(true))
        );
    }

    #[test]
    fn grip_unknown_type_falls_through_to_value() {
        // An object with an unrecognised "type" field is treated as a plain Value.
        let v = json!({"type": "future_type", "data": 1});
        assert_eq!(Grip::from_result_value(&v), Grip::Value(v));
    }

    #[test]
    fn grip_long_string_missing_actor_falls_through_to_value() {
        // longString without required "actor" falls through to Value.
        let v = json!({"type": "longString", "length": 100, "initial": "hi"});
        assert_eq!(Grip::from_result_value(&v), Grip::Value(v));
    }

    #[test]
    fn grip_object_missing_actor_falls_through_to_value() {
        // object without required "actor" falls through to Value.
        let v = json!({"type": "object", "class": "Array"});
        assert_eq!(Grip::from_result_value(&v), Grip::Value(v));
    }

    #[test]
    fn grip_object_missing_class_falls_through_to_value() {
        // object without required "class" falls through to Value.
        let v = json!({"type": "object", "actor": "server1.conn0.child1/obj1"});
        assert_eq!(Grip::from_result_value(&v), Grip::Value(v));
    }
}
