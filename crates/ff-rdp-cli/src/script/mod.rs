//! Script runner and recorder for ff-rdp.
//!
//! Provides:
//! - `format`: Script format types and parsing.
//! - `vars`: Variable substitution and secret redaction.
//! - `recorder`: File-based step recorder.
//! - `runner`: Step executor with NDJSON output.

pub mod format;
pub mod recorder;
pub mod runner;
pub mod vars;
