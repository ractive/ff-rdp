//! CSS helpers — specificity, cascade ordering, selector parsing.
//!
//! These are pure-Rust utilities shared by the `ff-rdp cascade` command and
//! any future style-inspection tooling.  They do not touch the RDP transport.

pub mod specificity;
