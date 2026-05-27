// Security: forbid any `unsafe` block in the core library.  All FFI / OS-level
// work lives in the CLI crate (daemon process management, script vars).  If a
// dependency or generated code ever introduces an `unsafe` here the build
// fails — see iter-75 (L-9).
#![forbid(unsafe_code)]

pub(crate) mod actor;
pub(crate) mod actors;
pub mod connection;
pub mod css;
pub mod error;
pub mod fronts;
pub mod registry;
pub mod resources;
pub mod session;
pub mod specs;
pub mod transport;
pub mod types;
pub mod util;

pub use actors::accessibility::{AccessibilityActor, AccessibleNode, filter_interactive};
pub use actors::console::{
    ConsoleMessage, EvalException, EvalResult, EvaluateScope, WebConsoleActor,
    parse_console_notification,
};
pub use actors::device::DeviceActor;
pub use actors::dom_walker::{DomAttr, DomNode, DomWalkerActor};
pub use actors::inspector::InspectorActor;
pub use actors::network::{EventTimings, Header, NetworkEventActor, ResponseContent};
pub use actors::object::{
    GripHandle, GripKind, LongStringGrip, LongStringScopedGrip, ObjectActor, ObjectGrip,
    ObjectScopedGrip, PropertyDescriptor, PrototypeAndProperties, ReleaseQueueRx, ReleaseQueueTx,
    ReleaseRequest, ScopedGrip, descriptor_to_json, release_queue,
};
pub use actors::page_style::{
    AppliedRule, BoxModelLayout, BoxSides, ComputedProperty, PageStyleActor, RuleProperty,
};
pub use actors::reflow::ReflowActor;
pub use actors::responsive::ResponsiveActor;
pub use actors::root::{ProcessInfo, RootActor};
pub use actors::screenshot::{ScreenshotActor, ScreenshotArgsExt, ScreenshotArgsRect};
pub use actors::screenshot_content::{
    CaptureRect, PrepareCapture, ScreenshotCapture, ScreenshotContentActor,
};
pub use actors::storage::{
    CookieInfo, NetworkSetCookie, StorageActor, merge_storage_and_network_cookies,
    parse_set_cookie_header,
};
pub use actors::string::LongStringActor;
pub use actors::tab::{TabActor, TabInfo, TargetInfo, note_tab_navigated_scheme_change};
pub use actors::target::WindowGlobalTarget;
pub use actors::thread::{SourceInfo, ThreadActor};
pub use actors::watcher::{
    ConsoleResource, NetworkResource, NetworkResourceUpdate, ResourceGripGuard, TargetEvent,
    WatcherActor, WatcherEvent, dispatch_watcher_event, extract_grips, parse_console_resources,
    parse_network_resource_updates, parse_network_resources, parse_target_event,
};
pub use connection::{COMPATIBLE_FIREFOX_MAX, COMPATIBLE_FIREFOX_MIN, RdpConnection};
pub use error::{ActorErrorKind, NavCause, ProtocolError, RdpError, RdpResult};
pub use fronts::{
    ConsoleFront, DescriptorFront, NetworkContentFront, PageStyleFront, ProcessDescriptorFront,
    ProcessTarget, RootFront, ScreenshotFront, TargetConfigurationFront, TargetFront, WalkerFront,
    WatcherFront,
};
pub use registry::{Front, FrontKind, IsActorGone, Registry, call_with_refresh};
pub use resources::{Resource, ResourceCommand, ResourceType, SubscriptionId};
pub use session::Session;
pub use transport::{DemuxReader, FramedReader, FramedWriter, Packet, RdpTransport};
pub use types::{ActorId, Grip};
pub use util::terminal::sanitize_for_terminal;

#[cfg(test)]
mod lib_attrs {
    /// AC: `core_lib_forbids_unsafe` — the `#![forbid(unsafe_code)]` attribute
    /// must be present at the top of `lib.rs`.  We pin it via a test that
    /// reads the source so a casual reformat or refactor cannot silently
    /// drop the guarantee.
    #[test]
    fn core_lib_forbids_unsafe() {
        let src = include_str!("lib.rs");
        assert!(
            src.contains("#![forbid(unsafe_code)]"),
            "lib.rs must contain `#![forbid(unsafe_code)]` (iter-75 L-9)"
        );
    }
}
