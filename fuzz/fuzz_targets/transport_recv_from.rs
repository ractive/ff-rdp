#![no_main]
//! Fuzz harness for the RDP length-prefixed frame parser.
//!
//! Wraps `ff_rdp_core::transport::recv_from`, which reads
//! `<length>:<json>` and `bulk <actor> <kind> <length>:<binary>` frames.
//! Attacker-controlled bytes ride on this surface from any compromised
//! Firefox/RDP peer, so panics here are security-relevant.

use std::io::Cursor;

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let mut cursor = Cursor::new(data);
    let _ = ff_rdp_core::transport::recv_from(&mut cursor);
});
