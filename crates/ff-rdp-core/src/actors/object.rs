use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::actor::actor_request;
use crate::error::ProtocolError;
use crate::transport::RdpTransport;
use crate::types::Grip;

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
}
