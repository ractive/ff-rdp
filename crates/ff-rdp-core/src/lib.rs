pub(crate) mod actor;
pub(crate) mod actors;
pub mod connection;
pub mod error;
pub mod transport;
pub mod types;

pub use actors::accessibility::{AccessibilityActor, AccessibleNode, filter_interactive};
pub use actors::console::{
    ConsoleMessage, EvalException, EvalResult, WebConsoleActor, parse_console_notification,
};
pub use actors::dom_walker::{DomAttr, DomNode, DomWalkerActor};
pub use actors::inspector::InspectorActor;
pub use actors::network::{EventTimings, Header, NetworkEventActor, ResponseContent};
pub use actors::object::{
    ObjectActor, PropertyDescriptor, PrototypeAndProperties, descriptor_to_json,
};
pub use actors::page_style::{
    AppliedRule, BoxModelLayout, BoxSides, ComputedProperty, PageStyleActor, RuleProperty,
};
pub use actors::responsive::ResponsiveActor;
pub use actors::root::RootActor;
pub use actors::screenshot::ScreenshotActor;
pub use actors::screenshot_content::{
    CaptureRect, PrepareCapture, ScreenshotCapture, ScreenshotContentActor,
};
pub use actors::storage::{CookieInfo, StorageActor};
pub use actors::string::LongStringActor;
pub use actors::tab::{TabActor, TabInfo, TargetInfo};
pub use actors::target::WindowGlobalTarget;
pub use actors::thread::{SourceInfo, ThreadActor};
pub use actors::watcher::{
    ConsoleResource, NetworkResource, NetworkResourceUpdate, TargetEvent, WatcherActor,
    parse_console_resources, parse_network_resource_updates, parse_network_resources,
    parse_target_event,
};
pub use connection::{COMPATIBLE_FIREFOX_MAX, COMPATIBLE_FIREFOX_MIN, RdpConnection};
pub use error::{ActorErrorKind, ProtocolError};
pub use transport::RdpTransport;
pub use types::{ActorId, Grip};
