//! Resource subscription abstractions for the Firefox Watcher actor.
//!
//! # Overview
//!
//! - [`ResourceType`] — the set of resource types the Watcher can deliver.
//! - [`Resource`] — a typed payload for one resource event.
//! - [`ResourceCommand`] — the central in-process subscription bus.

mod command;
mod resource;
mod resource_type;

pub use command::{ResourceCommand, SubscriptionId};
pub use resource::Resource;
pub use resource_type::ResourceType;
