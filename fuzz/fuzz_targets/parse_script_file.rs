#![no_main]
//! Fuzz harness for the script-format parser.
//!
//! Mirrors the public `parse_script_file` entry point by calling its
//! string-level inner, `parse_script_str`, on fuzzer-supplied bytes.

use libfuzzer_sys::fuzz_target;

#[allow(dead_code)]
#[path = "../../crates/ff-rdp-cli/src/script/format.rs"]
mod script_format;

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else {
        return;
    };
    let fmt = if s.as_bytes().first().is_some_and(|b| b % 2 == 0) {
        script_format::ScriptFormat::Json
    } else {
        script_format::ScriptFormat::Yaml
    };
    let _ = script_format::parse_script_str(s, fmt);
});
