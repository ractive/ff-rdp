//! Typed protocol layer — spec modules mirror Firefox DevTools actor specs.
//!
//! Each sub-module corresponds to one Firefox RDP actor and declares:
//! - `request::*` — typed args structs (serde `Serialize`)
//! - `response::*` — typed reply structs (serde `Deserialize`)
//! - Zero-sized method marker structs implementing [`Method`]
//!
//! The [`call`] helper takes care of serialize → [`crate::actor::actor_request`] → deserialize
//! in a single step, eliminating all `json!({...})` and `Value::as_*` calls from front code.

use crate::error::ProtocolError;
use crate::transport::RdpTransport;
use crate::types::ActorId;

pub mod console;
pub mod descriptor;
pub mod manifest;
pub mod network_event;
pub mod page_style;
pub mod root;
pub mod screenshot;
pub mod target;
pub mod types;
pub mod walker;
pub mod watcher;

// ---------------------------------------------------------------------------
// Sealed trait — prevents external crates from implementing `Method`.
// ---------------------------------------------------------------------------

mod sealed {
    pub trait Sealed {}
}

// ---------------------------------------------------------------------------
// Method trait
// ---------------------------------------------------------------------------

/// A typed RDP method marker.
///
/// Implement this on a zero-sized struct to describe one actor method:
/// its wire name, the request args type, and the response reply type.
///
/// Sealed so that only this crate can add new method markers.
pub trait Method: sealed::Sealed {
    /// The wire method name sent in the `"type"` field of the RDP request.
    const NAME: &'static str;
    /// Serde-serialisable request args.  Use `NoArgs` for methods with no parameters.
    type Args: serde::Serialize;
    /// Serde-deserialisable reply.
    type Reply: for<'de> serde::Deserialize<'de>;
    /// Whether this method is fire-and-forget (no reply packet expected).
    ///
    /// When `true`, [`call`] skips the reply read entirely and synthesizes a
    /// reply by deserializing an empty JSON object (`{}`).  The `Reply` type
    /// for oneway methods must therefore deserialize from `{}` — typically a
    /// unit struct with `#[derive(Deserialize)]`, or a struct whose fields all
    /// have `#[serde(default)]`.
    ///
    /// Set to `true` for methods that Firefox marks `oneway: true` in its spec,
    /// such as `unwatchTargets` and `clearResources`.  Defaults to `false`.
    const ONEWAY: bool = false;
}

/// Placeholder args for methods that take no parameters.
///
/// Serialises to `{}` — the empty JSON object that `actor_request` requires.
#[derive(Debug, Default, serde::Serialize)]
pub struct NoArgs {}

// ---------------------------------------------------------------------------
// Generic call helper
// ---------------------------------------------------------------------------

/// Serialise `args`, call [`crate::actor::actor_request`], deserialise the reply.
///
/// This is the single entry point used by all front methods.  It eliminates
/// manual `json!({...})` construction and `Value::as_*` parsing in front code.
///
/// When [`Method::ONEWAY`] is `true`, the request is sent but no reply is read.
/// The return value is synthesized by deserializing an empty JSON object (`{}`),
/// so `M::Reply` must deserialize successfully from `{}` — typically a unit
/// struct or a struct with all `#[serde(default)]` fields.
pub(crate) fn call<M: Method>(
    transport: &mut RdpTransport,
    actor_id: &ActorId,
    args: &M::Args,
) -> Result<M::Reply, ProtocolError> {
    let params = serde_json::to_value(args)
        .map_err(|e| ProtocolError::InvalidPacket(format!("encode {}: {e}", M::NAME)))?;
    if M::ONEWAY {
        crate::actor::actor_send(transport, actor_id.as_ref(), M::NAME, Some(&params))?;
        // No reply expected — construct the reply from an empty JSON object.
        // All oneway reply types are empty structs that accept `{}` via serde.
        serde_json::from_value::<M::Reply>(serde_json::json!({}))
            .map_err(|e| ProtocolError::InvalidPacket(format!("decode oneway {}: {e}", M::NAME)))
    } else {
        let response =
            crate::actor::actor_request(transport, actor_id.as_ref(), M::NAME, Some(&params))?;
        serde_json::from_value::<M::Reply>(response)
            .map_err(|e| ProtocolError::InvalidPacket(format!("decode {}: {e}", M::NAME)))
    }
}
