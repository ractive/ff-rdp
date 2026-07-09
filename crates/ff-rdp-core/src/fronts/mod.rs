//! Typed front handles — one per Firefox RDP actor kind.
//!
//! A *front* is a thin typed wrapper around an [`ActorId`] and a [`Registry`]
//! back-reference.  Every front implements the [`Front`] trait, which provides
//! the [`assert_alive`](crate::registry::Front::assert_alive) guard.
//!
//! Creating a front is O(1) — it does not touch the network.  All I/O goes
//! through the actor modules in `crate::actors`.

mod console;
mod descriptor;
mod manifest;
mod network_content;
mod page_style;
mod process_descriptor;
mod root;
mod screenshot;
mod target;
mod target_configuration;
mod walker;
mod watcher;

pub use console::ConsoleFront;
pub use descriptor::DescriptorFront;
pub use manifest::{CanonicalManifest, ManifestFront};
pub use network_content::NetworkContentFront;
pub use page_style::PageStyleFront;
pub use process_descriptor::{ProcessDescriptorFront, ProcessTarget};
pub use root::RootFront;
pub use screenshot::ScreenshotFront;
pub use target::TargetFront;
pub use target_configuration::TargetConfigurationFront;
pub use walker::WalkerFront;
pub use watcher::WatcherFront;
